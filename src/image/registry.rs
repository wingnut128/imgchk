use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use flate2::read::GzDecoder;

use crate::tree::FileTree;

use super::{
    BlobStore, CredentialResolver, DefaultCredentials, FsBlobStore, ImageConfig, ImageInfo,
    ImageSource, LayerInfo, parse_full_history, parse_history,
};

/// Loads images from OCI/Docker registries.
///
/// Holds injected [`CredentialResolver`] and [`BlobStore`] dependencies
/// so tests can substitute either in isolation.
pub struct RegistrySource {
    creds: Arc<dyn CredentialResolver>,
    blobs: Arc<dyn BlobStore>,
}

impl RegistrySource {
    pub fn new(creds: Arc<dyn CredentialResolver>, blobs: Arc<dyn BlobStore>) -> Self {
        Self { creds, blobs }
    }
}

impl Default for RegistrySource {
    fn default() -> Self {
        Self::new(Arc::new(DefaultCredentials), Arc::new(FsBlobStore::new()))
    }
}

impl ImageSource for RegistrySource {
    async fn load(&self, reference: &str, platform: Option<&str>) -> anyhow::Result<ImageInfo> {
        load(reference, platform, self.creds.as_ref(), self.blobs.clone()).await
    }
}

/// Parse a `"os/arch"` platform string (e.g. `"linux/arm64"`) into its two
/// halves, defaulting each independently: `None` (no `--platform` flag) or a
/// bare os with no `/arch` suffix both fall back to `amd64`.
fn parse_platform(platform: Option<&str>) -> (String, String) {
    let Some(p) = platform else {
        return ("linux".to_string(), "amd64".to_string());
    };
    let parts: Vec<&str> = p.split('/').collect();
    (
        parts.first().copied().unwrap_or("linux").to_string(),
        parts.get(1).copied().unwrap_or("amd64").to_string(),
    )
}

/// Pick the manifest-list entry matching `(os, arch)`, returning its digest.
/// This is the matching logic behind the `platform_resolver` callback
/// `oci_client` invokes when pulling a multi-arch image index; entries with
/// no `platform` field never match.
fn resolve_platform_digest(
    entries: &[oci_client::manifest::ImageIndexEntry],
    os: &str,
    arch: &str,
) -> Option<String> {
    entries
        .iter()
        .find(|entry| {
            entry
                .platform
                .as_ref()
                .is_some_and(|p| p.os.to_string() == os && p.architecture.to_string() == arch)
        })
        .map(|entry| entry.digest.clone())
}

async fn load(
    reference: &str,
    platform: Option<&str>,
    creds: &dyn CredentialResolver,
    blobs: Arc<dyn BlobStore>,
) -> anyhow::Result<ImageInfo> {
    use oci_client::Reference;
    use oci_client::client::{ClientConfig, ClientProtocol};
    use oci_client::manifest::ImageIndexEntry;

    let image_ref: Reference = reference.parse().context("invalid image reference")?;

    let (target_os, target_arch) = parse_platform(platform);

    let resolver_os = target_os.clone();
    let resolver_arch = target_arch.clone();
    let platform_resolver = move |entries: &[ImageIndexEntry]| -> Option<String> {
        resolve_platform_digest(entries, &resolver_os, &resolver_arch)
    };

    let config = ClientConfig {
        protocol: ClientProtocol::Https,
        platform_resolver: Some(Box::new(platform_resolver)),
        ..Default::default()
    };
    let client = oci_client::Client::new(config);
    let auth = creds.for_registry(image_ref.resolve_registry());

    use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

    let mp = MultiProgress::new();
    let spinner_style = ProgressStyle::with_template("{spinner:.cyan} {msg}")
        .unwrap()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ");

    let manifest_pb = mp.add(ProgressBar::new_spinner());
    manifest_pb.set_style(spinner_style.clone());
    manifest_pb.set_message(format!("Pulling manifest ({target_os}/{target_arch})..."));
    manifest_pb.enable_steady_tick(std::time::Duration::from_millis(80));

    let (manifest, _digest) = client
        .pull_image_manifest(&image_ref, &auth)
        .await
        .context("pulling manifest")?;

    manifest_pb.set_message("Pulling config...");

    let config_descriptor = &manifest.config;
    let mut config_bytes = Vec::new();
    client
        .pull_blob(&image_ref, config_descriptor, &mut config_bytes)
        .await
        .context("pulling config")?;

    manifest_pb.finish_and_clear();

    let image_config: ImageConfig = serde_json::from_slice(&config_bytes)?;

    let (commands, created_times) = parse_history(&image_config);
    let history = parse_full_history(&image_config);

    let diff_ids = image_config
        .rootfs
        .as_ref()
        .and_then(|r| r.diff_ids.clone())
        .unwrap_or_default();

    let tmp_dir = tempfile::tempdir().context("creating temp dir for layers")?;
    let layer_descriptors = manifest.layers.clone();

    let total_expected: u64 = layer_descriptors.iter().map(|d| d.size.max(0) as u64).sum();
    if !blobs.ensure_capacity(total_expected) {
        eprintln!(
            "Warning: may not have enough disk space for {} download",
            crate::tree::human_size(total_expected)
        );
    }

    let bar_style = ProgressStyle::with_template(
        "{spinner:.cyan} Layer {msg} [{bar:25.cyan/dim}] {bytes}/{total_bytes}",
    )
    .unwrap()
    .progress_chars("━╸─")
    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ");

    let layer_bars: Vec<ProgressBar> = layer_descriptors
        .iter()
        .enumerate()
        .map(|(i, desc)| {
            let size = desc.size.max(0) as u64;
            let pb = mp.add(ProgressBar::new(size));
            pb.set_style(bar_style.clone());
            pb.set_message(format!("{i}"));
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
            pb
        })
        .collect();

    let mut handles = Vec::new();
    for (i, desc) in layer_descriptors.iter().enumerate() {
        let image_ref = image_ref.clone();
        let desc = desc.clone();
        let tmp_path = tmp_dir.path().join(format!("layer-{i}.tar.gz"));
        let client = client.clone();
        let pb = layer_bars[i].clone();
        let blobs = blobs.clone();

        handles.push(tokio::spawn(async move {
            let expected_size = desc.size.max(0) as u64;

            if blobs.try_copy_cached(&desc.digest, expected_size, &tmp_path) {
                pb.set_message(format!("{i} (cached)"));
                pb.set_position(expected_size);
                pb.finish_and_clear();
                return Ok::<(PathBuf, u64), anyhow::Error>((tmp_path, expected_size));
            }

            let mut data = Vec::new();
            client
                .pull_blob(&image_ref, &desc, &mut data)
                .await
                .with_context(|| format!("pulling layer {i}"))?;

            let size = data.len() as u64;
            pb.set_position(size);
            pb.finish_and_clear();

            std::fs::write(&tmp_path, &data)?;
            blobs.store(&desc.digest, &data);

            Ok::<(PathBuf, u64), anyhow::Error>((tmp_path, size))
        }));
    }

    let mut layers = Vec::new();
    let mut total_size = 0u64;

    let parse_style = ProgressStyle::with_template("{spinner:.cyan} {msg}")
        .unwrap()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ");

    for (i, handle) in handles.into_iter().enumerate() {
        let (blob_path, size) = handle.await??;
        total_size += size;

        let parse_pb = mp.add(ProgressBar::new_spinner());
        parse_pb.set_style(parse_style.clone());
        parse_pb.set_message(format!(
            "Parsing layer {} ({})...",
            i,
            crate::tree::human_size(size)
        ));
        parse_pb.enable_steady_tick(std::time::Duration::from_millis(80));

        let file = std::fs::File::open(&blob_path)?;
        let decoder = GzDecoder::new(file);
        let file_tree = FileTree::from_tar(decoder)?;

        parse_pb.finish_and_clear();

        let digest = layer_descriptors[i].digest.clone();
        let diff_id = diff_ids.get(i).cloned().unwrap_or_default();
        let command = commands.get(i).cloned().unwrap_or_default();
        let created = created_times.get(i).cloned().unwrap_or_default();
        let media_type = layer_descriptors[i].media_type.clone();

        layers.push(LayerInfo {
            index: i,
            digest,
            diff_id,
            size,
            command,
            created,
            file_tree,
            blob_path,
            media_type,
        });
    }

    // Leak the tmpdir so blobs persist for the TUI session.
    let _ = tmp_dir.keep();

    Ok(ImageInfo {
        layers,
        total_size,
        architecture: image_config
            .architecture
            .unwrap_or_else(|| "unknown".into()),
        os: image_config.os.unwrap_or_else(|| "unknown".into()),
        source: reference.to_string(),
        history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oci_client::manifest::{ImageIndexEntry, Platform};
    use oci_spec::image::{Arch, Os};

    fn entry(os: Os, arch: Arch, digest: &str) -> ImageIndexEntry {
        ImageIndexEntry {
            media_type: "application/vnd.oci.image.manifest.v1+json".to_string(),
            digest: digest.to_string(),
            size: 0,
            platform: Some(Platform {
                architecture: arch,
                os,
                os_version: None,
                os_features: None,
                variant: None,
                features: None,
            }),
            annotations: None,
            artifact_type: None,
        }
    }

    #[test]
    fn parse_platform_defaults_to_linux_amd64_when_absent() {
        assert_eq!(
            parse_platform(None),
            ("linux".to_string(), "amd64".to_string())
        );
    }

    #[test]
    fn parse_platform_splits_os_and_arch() {
        assert_eq!(
            parse_platform(Some("linux/arm64")),
            ("linux".to_string(), "arm64".to_string())
        );
    }

    #[test]
    fn parse_platform_defaults_arch_when_slash_missing() {
        assert_eq!(
            parse_platform(Some("windows")),
            ("windows".to_string(), "amd64".to_string())
        );
    }

    #[test]
    fn resolve_platform_digest_finds_matching_entry() {
        let entries = vec![
            entry(Os::Linux, Arch::Amd64, "sha256:amd64digest"),
            entry(Os::Linux, Arch::ARM64, "sha256:arm64digest"),
        ];
        assert_eq!(
            resolve_platform_digest(&entries, "linux", "arm64"),
            Some("sha256:arm64digest".to_string())
        );
    }

    #[test]
    fn resolve_platform_digest_returns_none_when_no_match() {
        let entries = vec![entry(Os::Linux, Arch::Amd64, "sha256:amd64digest")];
        assert_eq!(resolve_platform_digest(&entries, "linux", "arm64"), None);
    }

    #[test]
    fn resolve_platform_digest_skips_entries_with_no_platform() {
        let mut e = entry(Os::Linux, Arch::Amd64, "sha256:x");
        e.platform = None;
        assert_eq!(resolve_platform_digest(&[e], "linux", "amd64"), None);
    }

    #[test]
    fn resolve_platform_digest_returns_first_match_on_duplicates() {
        let entries = vec![
            entry(Os::Linux, Arch::Amd64, "sha256:first"),
            entry(Os::Linux, Arch::Amd64, "sha256:second"),
        ];
        assert_eq!(
            resolve_platform_digest(&entries, "linux", "amd64"),
            Some("sha256:first".to_string())
        );
    }
}
