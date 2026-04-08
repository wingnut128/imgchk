use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use flate2::read::GzDecoder;
use serde::Deserialize;

use crate::tree::FileTree;

pub const MEDIA_TYPE_LAYER_GZIP: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";

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
    #[serde(rename = "Layers")]
    layers: Vec<String>,
}

// Image config (subset of fields we care about)
#[derive(Deserialize, Default)]
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

fn parse_history(config: &ImageConfig) -> (Vec<String>, Vec<String>) {
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
    (commands, created_times)
}

/// Load an image from a Docker-save tarball.
pub fn load_tarball(path: &Path) -> anyhow::Result<ImageInfo> {
    let file = std::fs::File::open(path).context("opening tarball")?;
    let mut archive = tar::Archive::new(file);

    let mut manifest_data: Option<Vec<u8>> = None;
    let mut config_data: Option<Vec<u8>> = None;
    let mut layer_data: std::collections::HashMap<String, Vec<u8>> =
        std::collections::HashMap::new();

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
        } else if path.ends_with("/layer.tar")
            || path.ends_with(".tar.gz")
            || path.ends_with(".tar")
        {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            layer_data.insert(path, buf);
        }
    }

    let manifest_bytes = manifest_data.context("manifest.json not found in tarball")?;
    let manifests: Vec<DockerManifest> = serde_json::from_slice(&manifest_bytes)?;
    let manifest = manifests.into_iter().next().context("empty manifest")?;

    let config: ImageConfig = config_data
        .as_deref()
        .and_then(|data| serde_json::from_slice(data).ok())
        .unwrap_or_default();

    let (commands, created_times) = parse_history(&config);

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
            media_type: MEDIA_TYPE_LAYER_GZIP.into(),
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

/// Docker config.json structure for reading credsStore/credHelpers.
#[derive(Deserialize, Default)]
struct DockerConfig {
    #[serde(rename = "credsStore")]
    creds_store: Option<String>,
    #[serde(rename = "credHelpers")]
    cred_helpers: Option<std::collections::HashMap<String, String>>,
}

/// Credential helper response.
#[derive(Deserialize)]
struct CredHelperResponse {
    #[serde(rename = "Username")]
    username: String,
    #[serde(rename = "Secret")]
    secret: String,
}

/// Resolve registry auth: env var first, then Docker credential store, then anonymous.
fn resolve_auth(registry: &str) -> oci_client::secrets::RegistryAuth {
    use oci_client::secrets::RegistryAuth;

    // 1. Check env var
    if let (Ok(user), Ok(token)) = (
        std::env::var("IMGCHK_REGISTRY_USER"),
        std::env::var("IMGCHK_REGISTRY_TOKEN"),
    ) {
        eprintln!("Using credentials from IMGCHK_REGISTRY_USER/TOKEN");
        return RegistryAuth::Basic(user, token);
    }

    // 2. Try Docker credential store
    if let Some(auth) = docker_credential(registry) {
        eprintln!("Using credentials from Docker credential store");
        return auth;
    }

    RegistryAuth::Anonymous
}

/// Read Docker's credential store for a given registry hostname.
fn docker_credential(registry: &str) -> Option<oci_client::secrets::RegistryAuth> {
    use oci_client::secrets::RegistryAuth;

    let config_path = dirs_path().join("config.json");
    let config_data = std::fs::read_to_string(&config_path).ok()?;
    let docker_config: DockerConfig = serde_json::from_str(&config_data).ok()?;

    // Map registry hostname to the credential store URL format
    let server_url = registry_to_server_url(registry);

    // Check per-registry credHelpers first, then global credsStore
    let helper_name = docker_config
        .cred_helpers
        .as_ref()
        .and_then(|h| h.get(registry).or_else(|| h.get(&server_url)))
        .cloned()
        .or(docker_config.creds_store)?;

    let helper_bin = format!("docker-credential-{}", helper_name);
    let output = Command::new(&helper_bin)
        .arg("get")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(server_url.as_bytes());
            }
            child.wait_with_output()
        })
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let cred: CredHelperResponse = serde_json::from_slice(&output.stdout).ok()?;
    Some(RegistryAuth::Basic(cred.username, cred.secret))
}

fn dirs_path() -> PathBuf {
    if let Ok(dir) = std::env::var("DOCKER_CONFIG") {
        PathBuf::from(dir)
    } else if let Some(home) = home_dir() {
        home.join(".docker")
    } else {
        PathBuf::from(".docker")
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

fn registry_to_server_url(registry: &str) -> String {
    match registry {
        "index.docker.io" | "registry-1.docker.io" | "docker.io" => {
            "https://index.docker.io/v1/".to_string()
        }
        other => format!("https://{}", other),
    }
}

/// Load an image from a registry reference (e.g., "nginx:latest").
pub async fn load_registry(reference: &str, platform: Option<&str>) -> anyhow::Result<ImageInfo> {
    use oci_client::client::{ClientConfig, ClientProtocol};
    use oci_client::manifest::ImageIndexEntry;
    use oci_client::Reference;

    let image_ref: Reference = reference.parse().context("invalid image reference")?;

    let (target_os, target_arch) = if let Some(p) = platform {
        let parts: Vec<&str> = p.split('/').collect();
        (
            parts.first().copied().unwrap_or("linux").to_string(),
            parts.get(1).copied().unwrap_or("amd64").to_string(),
        )
    } else {
        ("linux".to_string(), "amd64".to_string())
    };

    let resolver_os = target_os.clone();
    let resolver_arch = target_arch.clone();
    let platform_resolver = move |entries: &[ImageIndexEntry]| -> Option<String> {
        entries
            .iter()
            .find(|entry| {
                entry.platform.as_ref().is_some_and(|p| {
                    p.os.to_string() == resolver_os && p.architecture.to_string() == resolver_arch
                })
            })
            .map(|entry| entry.digest.clone())
    };

    let config = ClientConfig {
        protocol: ClientProtocol::Https,
        platform_resolver: Some(Box::new(platform_resolver)),
        ..Default::default()
    };
    let client = oci_client::Client::new(config);
    let auth = resolve_auth(image_ref.resolve_registry());

    use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

    let mp = MultiProgress::new();
    let spinner_style = ProgressStyle::with_template("{spinner:.cyan} {msg}")
        .unwrap()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ");

    let manifest_pb = mp.add(ProgressBar::new_spinner());
    manifest_pb.set_style(spinner_style.clone());
    manifest_pb.set_message(format!(
        "Pulling manifest ({}/{})...",
        target_os, target_arch
    ));
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

    let diff_ids = image_config
        .rootfs
        .as_ref()
        .and_then(|r| r.diff_ids.clone())
        .unwrap_or_default();

    // Download layers in parallel, using a disk cache for blobs
    let tmp_dir = tempfile::tempdir().context("creating temp dir for layers")?;
    let layer_descriptors = manifest.layers.clone();

    let cache_dir = crate::cache::cache_dir();

    // Check disk space before downloading
    let total_expected: u64 = layer_descriptors.iter().map(|d| d.size.max(0) as u64).sum();
    if !crate::cache::has_disk_space(&cache_dir, total_expected) {
        eprintln!(
            "Warning: may not have enough disk space for {} download",
            crate::tree::human_size(total_expected)
        );
    }

    // Evict old cache entries if over the limit
    crate::cache::evict_if_needed(&cache_dir);

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
            pb.set_message(format!("{}", i));
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
            pb
        })
        .collect();

    let mut handles = Vec::new();
    for (i, desc) in layer_descriptors.iter().enumerate() {
        let image_ref = image_ref.clone();
        let desc = desc.clone();
        let tmp_path = tmp_dir.path().join(format!("layer-{}.tar.gz", i));
        let client = client.clone();
        let pb = layer_bars[i].clone();
        let cache_dir = cache_dir.clone();

        handles.push(tokio::spawn(async move {
            let expected_size = desc.size.max(0) as u64;

            // Compute cache filename: "sha256:abcdef..." -> "sha256-abcdef..."
            let cache_filename = desc.digest.replace(':', "-");
            let cache_path = cache_dir.join(&cache_filename);

            // Check if blob is already cached with the correct size
            let cached = cache_path
                .metadata()
                .ok()
                .is_some_and(|m| m.len() == expected_size && expected_size > 0);

            if cached {
                pb.set_message(format!("{} (cached)", i));
                pb.set_position(expected_size);
                pb.finish_and_clear();

                crate::cache::touch(&cache_path);
                std::fs::copy(&cache_path, &tmp_path)
                    .with_context(|| format!("copying cached layer {} to tmpdir", i))?;
                Ok::<(PathBuf, u64), anyhow::Error>((tmp_path, expected_size))
            } else {
                let mut data = Vec::new();
                client
                    .pull_blob(&image_ref, &desc, &mut data)
                    .await
                    .with_context(|| format!("pulling layer {}", i))?;

                let size = data.len() as u64;
                pb.set_position(size);
                pb.finish_and_clear();

                std::fs::write(&tmp_path, &data)?;

                // Write to cache (best-effort: don't fail if cache write fails)
                if std::fs::create_dir_all(&cache_dir).is_ok() {
                    let tmp_cache = cache_dir.join(format!("{}.tmp", cache_filename));
                    if std::fs::write(&tmp_cache, &data).is_ok() {
                        let _ = std::fs::rename(&tmp_cache, &cache_path);
                    }
                }

                Ok::<(PathBuf, u64), anyhow::Error>((tmp_path, size))
            }
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

    // Leak the tmpdir so blobs persist for the TUI session
    let _ = tmp_dir.keep();

    Ok(ImageInfo {
        layers,
        total_size,
        architecture: image_config
            .architecture
            .unwrap_or_else(|| "unknown".into()),
        os: image_config.os.unwrap_or_else(|| "unknown".into()),
        source: reference.to_string(),
    })
}

/// Detect whether a source is a local tarball or a registry reference.
pub fn is_tarball(source: &str) -> bool {
    let path = Path::new(source);
    path.exists() && path.is_file()
}
