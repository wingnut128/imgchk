use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::read::GzDecoder;
use serde::Deserialize;

use crate::tree::FileTree;

/// Pre-parsed metadata for a single layer.
pub struct LayerInfo {
    pub index: usize,
    pub digest: String,
    pub diff_id: String,
    pub size: u64,
    pub command: String,
    pub created: String,
    pub file_tree: FileTree,
    pub blob_path: PathBuf,
    pub media_type: String,
}

/// Complete analyzed image.
pub struct ImageInfo {
    pub layers: Vec<LayerInfo>,
    pub total_size: u64,
    pub architecture: String,
    pub os: String,
    pub source: String,
}

// Docker save manifest.json format
#[derive(Deserialize)]
struct DockerManifest {
    #[serde(rename = "Config")]
    config: String,
    #[serde(rename = "Layers")]
    layers: Vec<String>,
    #[serde(rename = "RepoTags")]
    #[allow(dead_code)]
    repo_tags: Option<Vec<String>>,
}

// Image config (subset of fields we care about)
#[derive(Deserialize)]
struct ImageConfig {
    architecture: Option<String>,
    os: Option<String>,
    history: Option<Vec<HistoryEntry>>,
    rootfs: Option<RootFs>,
}

#[derive(Deserialize)]
struct HistoryEntry {
    created_by: Option<String>,
    created: Option<String>,
    empty_layer: Option<bool>,
}

#[derive(Deserialize)]
struct RootFs {
    diff_ids: Option<Vec<String>>,
}

/// Load an image from a Docker-save tarball.
pub fn load_tarball(path: &Path) -> anyhow::Result<ImageInfo> {
    let file = std::fs::File::open(path).context("opening tarball")?;
    let mut archive = tar::Archive::new(file);

    let mut manifest_data: Option<Vec<u8>> = None;
    let mut config_data: Option<Vec<u8>> = None;
    let mut layer_data: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();

    // First pass: read everything we need from the archive
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().to_string();

        if path == "manifest.json" {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            manifest_data = Some(buf);
        } else if path.ends_with(".json") && !path.contains('/') {
            // Config file (hash.json at root)
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            config_data = Some(buf);
        } else if path.ends_with("/layer.tar") || path.ends_with(".tar.gz") || path.ends_with(".tar") {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            layer_data.insert(path, buf);
        }
    }

    let manifest_bytes = manifest_data.context("manifest.json not found in tarball")?;
    let manifests: Vec<DockerManifest> = serde_json::from_slice(&manifest_bytes)?;
    let manifest = manifests.into_iter().next().context("empty manifest")?;

    // Re-read config if we grabbed the wrong json
    let config: ImageConfig = if let Some(ref data) = config_data {
        serde_json::from_slice(data).unwrap_or_else(|_| ImageConfig {
            architecture: None,
            os: None,
            history: None,
            rootfs: None,
        })
    } else {
        ImageConfig {
            architecture: None,
            os: None,
            history: None,
            rootfs: None,
        }
    };

    // Extract commands from history (skip empty layers)
    let mut commands = Vec::new();
    let mut created_times = Vec::new();
    if let Some(history) = &config.history {
        for h in history {
            if h.empty_layer.unwrap_or(false) {
                continue;
            }
            commands.push(h.created_by.clone().unwrap_or_default());
            created_times.push(h.created.clone().unwrap_or_default());
        }
    }

    let diff_ids = config
        .rootfs
        .as_ref()
        .and_then(|r| r.diff_ids.clone())
        .unwrap_or_default();

    let mut layers = Vec::new();
    let mut total_size = 0u64;

    for (i, layer_path) in manifest.layers.iter().enumerate() {
        let data = layer_data
            .get(layer_path)
            .with_context(|| format!("layer {} not found in tarball", layer_path))?;

        let size = data.len() as u64;
        total_size += size;

        // Try to decompress as gzip, fall back to raw tar
        let file_tree = if layer_path.ends_with(".gz") || layer_path.ends_with(".tar.gz") {
            let decoder = GzDecoder::new(std::io::Cursor::new(data));
            FileTree::from_tar(decoder)?
        } else {
            FileTree::from_tar(std::io::Cursor::new(data))?
        };

        let command = commands.get(i).cloned().unwrap_or_default();
        let created = created_times.get(i).cloned().unwrap_or_default();
        let diff_id = diff_ids.get(i).cloned().unwrap_or_default();

        layers.push(LayerInfo {
            index: i,
            digest: format!("sha256:{}", layer_path.replace("/layer.tar", "")),
            diff_id,
            size,
            command,
            created,
            file_tree,
            blob_path: PathBuf::from(layer_path),
            media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".into(),
        });
    }

    Ok(ImageInfo {
        layers,
        total_size,
        architecture: config.architecture.unwrap_or_else(|| "unknown".into()),
        os: config.os.unwrap_or_else(|| "unknown".into()),
        source: path.display().to_string(),
    })
}

/// Load an image from a registry reference (e.g., "nginx:latest").
pub async fn load_registry(reference: &str, platform: Option<&str>) -> anyhow::Result<ImageInfo> {
    use oci_distribution::client::{ClientConfig, ClientProtocol};
    use oci_distribution::secrets::RegistryAuth;
    use oci_distribution::Reference;
    use sha2::{Digest, Sha256};

    let image_ref: Reference = reference
        .parse()
        .context("invalid image reference")?;

    let config = ClientConfig {
        protocol: ClientProtocol::Https,
        ..Default::default()
    };
    let client = oci_distribution::Client::new(config);
    let auth = RegistryAuth::Anonymous;

    eprintln!("Pulling manifest...");
    let (manifest, _digest) = client
        .pull_image_manifest(&image_ref, &auth)
        .await
        .context("pulling manifest")?;

    // Pull the config blob
    let config_descriptor = &manifest.config;
    let mut config_bytes = Vec::new();
    client
        .pull_blob(&image_ref, config_descriptor, &mut config_bytes)
        .await
        .context("pulling config")?;

    let image_config: ImageConfig = serde_json::from_slice(&config_bytes)?;

    // Extract commands from history
    let mut commands = Vec::new();
    let mut created_times = Vec::new();
    if let Some(history) = &image_config.history {
        for h in history {
            if h.empty_layer.unwrap_or(false) {
                continue;
            }
            commands.push(h.created_by.clone().unwrap_or_default());
            created_times.push(h.created.clone().unwrap_or_default());
        }
    }

    let diff_ids = image_config
        .rootfs
        .as_ref()
        .and_then(|r| r.diff_ids.clone())
        .unwrap_or_default();

    // Parse platform
    let (_os_filter, _arch_filter) = if let Some(p) = platform {
        let parts: Vec<&str> = p.split('/').collect();
        (
            parts.first().copied().unwrap_or("linux"),
            parts.get(1).copied().unwrap_or("amd64"),
        )
    } else {
        ("linux", "amd64")
    };

    // Download layers in parallel
    let tmp_dir = tempfile::tempdir().context("creating temp dir for layers")?;
    let layer_descriptors = manifest.layers.clone();

    eprintln!("Downloading {} layers...", layer_descriptors.len());

    let mut handles = Vec::new();
    for (i, desc) in layer_descriptors.iter().enumerate() {
        let image_ref = image_ref.clone();
        let desc = desc.clone();
        let tmp_path = tmp_dir.path().join(format!("layer-{}.tar.gz", i));
        let client_config = ClientConfig {
            protocol: ClientProtocol::Https,
            ..Default::default()
        };

        handles.push(tokio::spawn(async move {
            let client = oci_distribution::Client::new(client_config);
            let mut data = Vec::new();
            client
                .pull_blob(&image_ref, &desc, &mut data)
                .await
                .with_context(|| format!("pulling layer {}", i))?;

            // Compute sha256
            let mut hasher = Sha256::new();
            hasher.update(&data);
            let hash = hex::encode(hasher.finalize());

            std::fs::write(&tmp_path, &data)?;
            Ok::<(PathBuf, u64, String), anyhow::Error>((tmp_path, data.len() as u64, hash))
        }));
    }

    let mut layers = Vec::new();
    let mut total_size = 0u64;

    for (i, handle) in handles.into_iter().enumerate() {
        let (blob_path, size, _hash) = handle.await??;
        total_size += size;

        eprintln!("  Layer {} ({})...", i, crate::tree::human_size(size));

        // Parse file tree from the layer
        let file = std::fs::File::open(&blob_path)?;
        let decoder = GzDecoder::new(file);
        let file_tree = FileTree::from_tar(decoder)?;

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

    // Leak the tmpdir so blobs persist for the TUI session
    let _ = tmp_dir.keep();

    Ok(ImageInfo {
        layers,
        total_size,
        architecture: image_config.architecture.unwrap_or_else(|| "unknown".into()),
        os: image_config.os.unwrap_or_else(|| "unknown".into()),
        source: reference.to_string(),
    })
}

/// Detect whether a source is a local tarball or a registry reference.
pub fn is_tarball(source: &str) -> bool {
    let path = Path::new(source);
    path.exists() && path.is_file()
}
