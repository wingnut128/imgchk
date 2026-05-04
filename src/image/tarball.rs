use std::io::{Read, Seek, SeekFrom};
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

/// Loads images from `docker save` tarballs and single-layer tar(.gz) files
/// on the local filesystem.
pub struct TarballSource;

impl ImageSource for TarballSource {
    async fn load(&self, reference: &str, _platform: Option<&str>) -> anyhow::Result<ImageInfo> {
        load_tarball(Path::new(reference))
    }
}

/// Open `path` as a streaming tar reader, transparently wrapping the file in
/// `GzDecoder` when its first two bytes are the gzip magic `1f 8b`.
fn open_tar(path: &Path) -> anyhow::Result<Box<dyn Read>> {
    let mut file = std::fs::File::open(path).context("opening tarball")?;
    let mut magic = [0u8; 2];
    let n = file.read(&mut magic).context("reading magic bytes")?;
    file.seek(SeekFrom::Start(0)).context("rewinding tarball")?;
    if n == 2 && magic == [0x1f, 0x8b] {
        Ok(Box::new(GzDecoder::new(file)))
    } else {
        Ok(Box::new(file))
    }
}

/// True if the (possibly gzipped) tar at `path` has a top-level `manifest.json`.
fn has_docker_manifest(path: &Path) -> anyhow::Result<bool> {
    let reader = open_tar(path)?;
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.path()?.to_string_lossy() == "manifest.json" {
            return Ok(true);
        }
    }
    Ok(false)
}

fn load_tarball(path: &Path) -> anyhow::Result<ImageInfo> {
    if has_docker_manifest(path)? {
        load_docker_archive(path)
    } else {
        load_single_layer(path)
    }
}

fn load_docker_archive(path: &Path) -> anyhow::Result<ImageInfo> {
    let reader = open_tar(path)?;
    let mut archive = tar::Archive::new(reader);

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

/// Load a bare layer tar(.gz) — no `manifest.json`, no image config — as a
/// degenerate one-layer `ImageInfo`. Metadata fields that don't apply
/// (digest, diff_id, command, created) are blank or sentinel.
fn load_single_layer(path: &Path) -> anyhow::Result<ImageInfo> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .len();
    let reader = open_tar(path)?;
    let file_tree = FileTree::from_tar(reader)
        .with_context(|| format!("reading tar entries from {}", path.display()))?;

    let layer = LayerInfo {
        index: 0,
        digest: "(unavailable — single-layer tarball)".into(),
        diff_id: String::new(),
        size,
        command: String::new(),
        created: String::new(),
        file_tree,
        blob_path: path.to_path_buf(),
        media_type: MEDIA_TYPE_LAYER_GZIP.into(),
    };

    Ok(ImageInfo {
        layers: vec![layer],
        total_size: size,
        architecture: "unknown".into(),
        os: "unknown".into(),
        source: path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    fn write_single_file_tar() -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let payload = b"hello\n";
        let mut header = tar::Header::new_gnu();
        header.set_path("usr/bin/hello").unwrap();
        header.set_size(payload.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append(&header, &payload[..]).unwrap();
        builder.into_inner().unwrap()
    }

    fn write_docker_archive() -> Vec<u8> {
        let layer_tar = write_single_file_tar();
        let manifest = br#"[{"Config":"config.json","RepoTags":[],"Layers":["layer0/layer.tar"]}]"#;
        let config = br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":["sha256:abc"]}}"#;

        let mut builder = tar::Builder::new(Vec::new());

        let mut h = tar::Header::new_gnu();
        h.set_path("manifest.json").unwrap();
        h.set_size(manifest.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        builder.append(&h, &manifest[..]).unwrap();

        let mut h = tar::Header::new_gnu();
        h.set_path("config.json").unwrap();
        h.set_size(config.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        builder.append(&h, &config[..]).unwrap();

        let mut h = tar::Header::new_gnu();
        h.set_path("layer0/layer.tar").unwrap();
        h.set_size(layer_tar.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        builder.append(&h, &layer_tar[..]).unwrap();

        builder.into_inner().unwrap()
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    fn write_temp(bytes: &[u8], suffix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("imgchk-tarball-test-{pid}-{nanos}{suffix}"));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn loads_uncompressed_docker_archive() {
        let path = write_temp(&write_docker_archive(), ".tar");
        let info = load_tarball(&path).unwrap();
        assert_eq!(info.layers.len(), 1);
        assert_eq!(info.architecture, "amd64");
        assert_eq!(info.os, "linux");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loads_gzipped_docker_archive() {
        let path = write_temp(&gzip(&write_docker_archive()), ".tar.gz");
        let info = load_tarball(&path).unwrap();
        assert_eq!(info.layers.len(), 1);
        assert_eq!(info.architecture, "amd64");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loads_single_layer_tar_gz() {
        let path = write_temp(&gzip(&write_single_file_tar()), ".tar.gz");
        let info = load_tarball(&path).unwrap();
        assert_eq!(info.layers.len(), 1);
        let layer = &info.layers[0];
        assert!(layer.digest.contains("unavailable"));
        assert_eq!(layer.command, "");
        assert_eq!(layer.blob_path, path);
        assert!(layer.file_tree.file_count >= 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loads_single_layer_uncompressed_tar() {
        let path = write_temp(&write_single_file_tar(), ".tar");
        let info = load_tarball(&path).unwrap();
        assert_eq!(info.layers.len(), 1);
        assert!(info.layers[0].file_tree.file_count >= 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_random_garbage() {
        let path = write_temp(b"not a tarball at all, just bytes", ".bin");
        let result = load_tarball(&path);
        assert!(result.is_err(), "expected error, got {:?}", result.is_ok());
        let _ = std::fs::remove_file(&path);
    }
}
