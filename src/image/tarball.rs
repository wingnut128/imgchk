use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::read::GzDecoder;
use serde::Deserialize;

use crate::tree::FileTree;

use super::{ImageConfig, ImageInfo, ImageSource, LayerInfo, MEDIA_TYPE_LAYER_GZIP, parse_history};

#[derive(Deserialize)]
struct DockerManifest {
    #[serde(rename = "Layers")]
    layers: Vec<String>,
}

/// Loads images from `docker save` tarballs on the local filesystem.
pub struct TarballSource;

impl ImageSource for TarballSource {
    async fn load(&self, reference: &str, _platform: Option<&str>) -> anyhow::Result<ImageInfo> {
        load_tarball(Path::new(reference))
    }
}

fn load_tarball(path: &Path) -> anyhow::Result<ImageInfo> {
    let file = std::fs::File::open(path).context("opening tarball")?;
    let mut archive = tar::Archive::new(file);

    let mut manifest_data: Option<Vec<u8>> = None;
    let mut config_data: Option<Vec<u8>> = None;
    let mut layer_data: std::collections::HashMap<String, Vec<u8>> =
        std::collections::HashMap::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().to_string();

        if path == "manifest.json" {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            manifest_data = Some(buf);
        } else if path.ends_with(".json") && !path.contains('/') {
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
