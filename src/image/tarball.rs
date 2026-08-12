use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::Context;
use flate2::read::GzDecoder;
use serde::Deserialize;

use crate::tree::FileTree;

use super::{
    ImageConfig, ImageInfo, ImageSource, LayerInfo, MEDIA_TYPE_LAYER_GZIP, MEDIA_TYPE_LAYER_TAR,
    parse_full_history, parse_history,
};

#[derive(Deserialize)]
struct DockerManifest {
    #[serde(rename = "Layers")]
    layers: Vec<String>,
    /// Path of the image config within the archive. Modern `docker save` emits
    /// an OCI layout where this is `blobs/sha256/<hex>`; the legacy format used
    /// a top-level `<hex>.json`.
    #[serde(rename = "Config")]
    config: Option<String>,
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

/// True if the file at `path` starts with the gzip magic `1f 8b`.
fn is_gzip_file(path: &Path) -> anyhow::Result<bool> {
    let mut file = std::fs::File::open(path).context("opening tarball")?;
    let mut magic = [0u8; 2];
    let n = file.read(&mut magic).context("reading magic bytes")?;
    Ok(n == 2 && magic == [0x1f, 0x8b])
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
    let mut top_level_json: Option<Vec<u8>> = None;
    // Keyed by the entry's path within the archive, so manifest.json's `Layers`
    // and `Config` entries can be resolved by exact path. Layer blobs are named
    // differently per format — `layer0/layer.tar` (legacy) vs. an extensionless
    // `blobs/sha256/<hex>` (the OCI layout modern `docker save` emits) — so
    // every regular entry is retained rather than filtered by filename shape.
    let mut blobs: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let path = entry.path()?.to_string_lossy().to_string();

        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;

        if path == "manifest.json" {
            manifest_data = Some(buf);
        } else if path == "oci-layout" || path == "index.json" {
            // OCI layout bookkeeping, not image content.
        } else {
            if path.ends_with(".json") && !path.contains('/') {
                top_level_json = Some(buf.clone());
            }
            blobs.insert(path, buf);
        }
    }

    let manifest_bytes = manifest_data.context("manifest.json not found in tarball")?;
    let manifests: Vec<DockerManifest> = serde_json::from_slice(&manifest_bytes)?;
    let manifest = manifests.into_iter().next().context("empty manifest")?;

    // Prefer the config the manifest points at; fall back to a top-level
    // `<hex>.json` for legacy archives whose manifest omits `Config`.
    let config_data = manifest
        .config
        .as_ref()
        .and_then(|p| blobs.get(p))
        .or(top_level_json.as_ref());

    let config: ImageConfig = config_data
        .and_then(|data| serde_json::from_slice(data).ok())
        .unwrap_or_default();

    let (commands, created_times) = parse_history(&config);
    let history = parse_full_history(&config);

    let diff_ids = config
        .rootfs
        .as_ref()
        .and_then(|r| r.diff_ids.clone())
        .unwrap_or_default();

    // Create temp directory for layer blobs (mirrors registry.rs pattern).
    let tmp_dir = tempfile::tempdir().context("creating temp dir for layers")?;

    let mut layers = Vec::new();
    let mut total_size = 0u64;

    for (i, layer_path) in manifest.layers.iter().enumerate() {
        let data = blobs
            .get(layer_path)
            .with_context(|| format!("layer {layer_path} not found in tarball"))?;

        let size = data.len() as u64;
        total_size += size;

        // Legacy `layer0/layer.tar` entries are plain tars, while OCI-layout
        // blobs are gzipped despite carrying no `.gz` suffix — so the encoding
        // is detected from the gzip magic, never from the name. The blob
        // filename and media type must both match it, since extraction decides
        // whether to gunzip from either one.
        let is_gzip = data.starts_with(&[0x1f, 0x8b]);

        let file_tree = if is_gzip {
            let decoder = GzDecoder::new(std::io::Cursor::new(data));
            FileTree::from_tar(decoder)?
        } else {
            FileTree::from_tar(std::io::Cursor::new(data))?
        };

        let command = commands.get(i).cloned().unwrap_or_default();
        let created = created_times.get(i).cloned().unwrap_or_default();
        let diff_id = diff_ids.get(i).cloned().unwrap_or_default();

        // Write layer data to temp file (mirrors registry.rs pattern).
        let ext = if is_gzip { "tar.gz" } else { "tar" };
        let blob_path = tmp_dir.path().join(format!("layer-{i}.{ext}"));
        std::fs::write(&blob_path, data)
            .with_context(|| format!("writing layer blob: {}", blob_path.display()))?;

        layers.push(LayerInfo {
            index: i,
            // Legacy entries are `<hex>/layer.tar`; OCI-layout entries are
            // `blobs/sha256/<hex>`. Both carry the digest in a path segment.
            digest: format!(
                "sha256:{}",
                layer_path
                    .strip_suffix("/layer.tar")
                    .unwrap_or(layer_path)
                    .rsplit('/')
                    .next()
                    .unwrap_or(layer_path)
            ),
            diff_id,
            size,
            command,
            created,
            file_tree,
            blob_path,
            media_type: if is_gzip {
                MEDIA_TYPE_LAYER_GZIP.into()
            } else {
                MEDIA_TYPE_LAYER_TAR.into()
            },
        });
    }

    Ok(ImageInfo {
        layers,
        total_size,
        architecture: config.architecture.unwrap_or_else(|| "unknown".into()),
        os: config.os.unwrap_or_else(|| "unknown".into()),
        source: path.display().to_string(),
        history,
        // Hand the staging dir to the ImageInfo so the blobs live as long as the
        // layers that point at them, and are removed when it drops.
        blob_dir: Some(tmp_dir),
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
        // The blob is the file itself, so the media type must reflect its actual
        // encoding — extraction gunzips based on it.
        media_type: if is_gzip_file(path)? {
            MEDIA_TYPE_LAYER_GZIP.into()
        } else {
            MEDIA_TYPE_LAYER_TAR.into()
        },
    };

    Ok(ImageInfo {
        layers: vec![layer],
        total_size: size,
        architecture: "unknown".into(),
        os: "unknown".into(),
        source: path.display().to_string(),
        history: Vec::new(),
        // The blob is the user's own file, read in place — nothing to clean up.
        blob_dir: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use std::path::PathBuf;

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

    /// An OCI-layout archive in the shape modern `docker save` emits: gzipped,
    /// extensionless `blobs/sha256/<hex>` layers, a config addressed the same
    /// way, and `oci-layout` / `index.json` bookkeeping entries.
    fn write_oci_layout_archive() -> Vec<u8> {
        let layer_blob = gzip(&write_single_file_tar());
        let config = br#"{"architecture":"arm64","os":"linux","rootfs":{"type":"layers","diff_ids":["sha256:aaa"]},"history":[{"created_by":"RUN true"}]}"#;
        let layer_digest = "b".repeat(64);
        let config_digest = "c".repeat(64);

        let manifest = format!(
            r#"[{{"Config":"blobs/sha256/{config_digest}","RepoTags":["x:latest"],"Layers":["blobs/sha256/{layer_digest}"]}}]"#
        );

        let mut builder = tar::Builder::new(Vec::new());
        let mut add = |path: &str, bytes: &[u8]| {
            let mut h = tar::Header::new_gnu();
            h.set_path(path).unwrap();
            h.set_size(bytes.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            builder.append(&h, bytes).unwrap();
        };

        add("oci-layout", br#"{"imageLayoutVersion":"1.0.0"}"#);
        add("index.json", br#"{"schemaVersion":2,"manifests":[]}"#);
        add("manifest.json", manifest.as_bytes());
        add(&format!("blobs/sha256/{config_digest}"), config);
        add(&format!("blobs/sha256/{layer_digest}"), &layer_blob);

        builder.into_inner().unwrap()
    }

    #[test]
    fn loads_oci_layout_docker_archive() {
        let path = write_temp(&write_oci_layout_archive(), ".tar");
        let info = load_tarball(&path).unwrap();

        // Config must be resolved via the manifest's `Config` path, not a
        // top-level *.json (there isn't one in this layout).
        assert_eq!(info.architecture, "arm64");
        assert_eq!(info.os, "linux");
        assert_eq!(info.layers.len(), 1);

        let layer = &info.layers[0];
        // Digest comes from the blob path's last segment, not the whole path.
        assert_eq!(layer.digest, format!("sha256:{}", "b".repeat(64)));
        // Gzip detected from magic bytes despite the extensionless entry name.
        assert_eq!(layer.media_type, MEDIA_TYPE_LAYER_GZIP);
        assert_eq!(layer.blob_path.extension().unwrap(), "gz");
        assert!(layer.file_tree.root.children.contains_key("usr"));

        // And the blob on disk must decode the way extract_with decodes it.
        let file = std::fs::File::open(&layer.blob_path).unwrap();
        let mut archive = tar::Archive::new(GzDecoder::new(file));
        let mut entries = 0;
        for entry in archive.entries().unwrap() {
            entry.unwrap();
            entries += 1;
        }
        assert_eq!(entries, 1);

        let _ = std::fs::remove_file(&path);
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
        use std::sync::atomic::{AtomicU64, Ordering};

        // Tests run in parallel and share this process's PID, so a timestamp
        // alone can collide when two `write_temp` calls land in the same tick.
        // A process-wide counter guarantees each temp file gets a unique name.
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let mut path = std::env::temp_dir();
        let pid = std::process::id();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("imgchk-tarball-test-{pid}-{seq}{suffix}"));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn single_layer_media_type_matches_actual_encoding() {
        // A plain tar must not be labelled gzip, or extraction gunzips a raw stream.
        let plain = write_temp(&write_single_file_tar(), ".tar");
        let info = load_tarball(&plain).unwrap();
        assert_eq!(info.layers[0].media_type, MEDIA_TYPE_LAYER_TAR);
        let _ = std::fs::remove_file(&plain);

        let gzipped = write_temp(&gzip(&write_single_file_tar()), ".tar.gz");
        let info = load_tarball(&gzipped).unwrap();
        assert_eq!(info.layers[0].media_type, MEDIA_TYPE_LAYER_GZIP);
        let _ = std::fs::remove_file(&gzipped);
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
    fn docker_archive_layer_tree_has_expected_file() {
        let path = write_temp(&write_docker_archive(), ".tar");
        let info = load_tarball(&path).unwrap();
        let tree = &info.layers[0].file_tree;

        let node = tree
            .find("usr/bin/hello")
            .expect("usr/bin/hello should be present in the layer tree");
        assert!(!node.is_dir);
        assert_eq!(node.path, "/usr/bin/hello");
        assert_eq!(node.size, b"hello\n".len() as u64);
        assert_eq!(node.mode & 0o777, 0o755);

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

    #[test]
    fn docker_archive_layer_blob_paths_exist_on_disk() {
        // Create a docker archive with two layers to test the fix.
        let layer1_tar = write_single_file_tar();

        // Create a second layer with a different file.
        let mut layer2_builder = tar::Builder::new(Vec::new());
        let payload2 = b"world\n";
        let mut header2 = tar::Header::new_gnu();
        header2.set_path("usr/bin/world").unwrap();
        header2.set_size(payload2.len() as u64);
        header2.set_mode(0o755);
        header2.set_cksum();
        layer2_builder.append(&header2, &payload2[..]).unwrap();
        let layer2_tar = layer2_builder.into_inner().unwrap();

        let manifest =
            br#"[{"Config":"config.json","RepoTags":[],"Layers":["layer0/layer.tar","layer1/layer.tar"]}]"#;
        let config = br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":["sha256:abc","sha256:def"]}}"#;

        let mut builder = tar::Builder::new(Vec::new());

        // Add manifest.json
        let mut h = tar::Header::new_gnu();
        h.set_path("manifest.json").unwrap();
        h.set_size(manifest.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        builder.append(&h, &manifest[..]).unwrap();

        // Add config.json
        let mut h = tar::Header::new_gnu();
        h.set_path("config.json").unwrap();
        h.set_size(config.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        builder.append(&h, &config[..]).unwrap();

        // Add first layer
        let mut h = tar::Header::new_gnu();
        h.set_path("layer0/layer.tar").unwrap();
        h.set_size(layer1_tar.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        builder.append(&h, &layer1_tar[..]).unwrap();

        // Add second layer
        let mut h = tar::Header::new_gnu();
        h.set_path("layer1/layer.tar").unwrap();
        h.set_size(layer2_tar.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        builder.append(&h, &layer2_tar[..]).unwrap();

        let archive_bytes = builder.into_inner().unwrap();
        let path = write_temp(&archive_bytes, ".tar");

        let info = load_tarball(&path).unwrap();
        assert_eq!(info.layers.len(), 2);

        // Critical test: blob_path must be a real filesystem path that exists and is readable.
        for (i, layer) in info.layers.iter().enumerate() {
            assert!(
                layer.blob_path.exists(),
                "Layer {} blob_path does not exist: {}",
                i,
                layer.blob_path.display()
            );

            // The blob must be decodable exactly the way extract_with decodes it:
            // gunzip when the media type says gzip or the extension is .gz, else raw.
            let file = std::fs::File::open(&layer.blob_path)
                .unwrap_or_else(|e| panic!("Failed to open layer {} blob_path: {}", i, e));
            assert!(
                file.metadata().unwrap().len() > 0,
                "Layer {} blob_path is empty",
                i
            );

            let reader: Box<dyn Read> = if layer.media_type.contains("gzip")
                || layer.blob_path.extension().is_some_and(|e| e == "gz")
            {
                Box::new(GzDecoder::new(file))
            } else {
                Box::new(file)
            };
            // Every entry must parse — `entries()` yields Err lazily, so counting
            // without unwrapping would pass even on a mis-encoded blob.
            let mut archive = tar::Archive::new(reader);
            let mut entries = 0;
            for entry in archive
                .entries()
                .unwrap_or_else(|e| panic!("Layer {i} blob is not a readable tar: {e}"))
            {
                entry.unwrap_or_else(|e| panic!("Layer {i} blob has an unreadable tar entry: {e}"));
                entries += 1;
            }
            assert!(entries > 0, "Layer {i} blob decoded to an empty archive");
        }

        let _ = std::fs::remove_file(&path);
    }
}
