use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use ocirender::{ImageSpec, LayerMeta, StreamingPacker};

use crate::image::LayerInfo;

mod path_safety;
mod selector;
mod writer;

pub use path_safety::safe_path;
pub use selector::{FileSelector, SelectedSet};
pub use writer::{DirWriter, OutputWriter, TarGzWriter, TarWriter};

// ── Output format ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputFormat {
    TarGz,
    Tar,
    Squashfs,
    Dir,
}

impl OutputFormat {
    pub fn next(self) -> Self {
        match self {
            Self::TarGz => Self::Tar,
            Self::Tar => Self::Squashfs,
            Self::Squashfs => Self::Dir,
            Self::Dir => Self::TarGz,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TarGz => "tar.gz",
            Self::Tar => "tar",
            Self::Squashfs => "squashfs",
            Self::Dir => "dir",
        }
    }
}

/// Build an ImageSpec for the given format and output directory.
pub fn make_image_spec(format: OutputFormat, output_dir: &Path, name: &str) -> ImageSpec {
    match format {
        OutputFormat::Tar => ImageSpec::Tar {
            path: output_dir.join(format!("{name}.tar")),
        },
        OutputFormat::Squashfs => ImageSpec::Squashfs {
            path: output_dir.join(format!("{name}.squashfs")),
            binpath: None,
        },
        OutputFormat::Dir => ImageSpec::Dir {
            path: output_dir.join(name),
        },
        OutputFormat::TarGz => unreachable!("TarGz uses the existing export path"),
    }
}

/// Export all layers merged into a single output via ocirender's StreamingPacker.
pub fn export_ocirender(layers: &[LayerInfo], spec: ImageSpec) -> anyhow::Result<PathBuf> {
    let metas: Vec<LayerMeta> = layers
        .iter()
        .enumerate()
        .map(|(i, l)| LayerMeta {
            index: i,
            media_type: l.media_type.clone(),
        })
        .collect();

    let output_path = spec.path().to_path_buf();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let packer = StreamingPacker::new(metas, spec, None);
        for (i, layer) in layers.iter().enumerate() {
            packer
                .notify_layer_ready(i, layer.blob_path.clone())
                .await?;
        }
        packer.finish().await
    })?;

    Ok(output_path)
}

/// Export a single layer via ocirender (re-indexes to 0).
pub fn export_ocirender_single(layer: &LayerInfo, spec: ImageSpec) -> anyhow::Result<PathBuf> {
    let meta = LayerMeta {
        index: 0,
        media_type: layer.media_type.clone(),
    };
    let output_path = spec.path().to_path_buf();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let packer = StreamingPacker::new(vec![meta], spec, None);
        packer
            .notify_layer_ready(0, layer.blob_path.clone())
            .await?;
        packer.finish().await
    })?;

    Ok(output_path)
}

/// Walk a layer's tar entries, applying `selector` and the path-safety
/// predicate, and route matches to `writer`. Returns the count actually
/// written and the paths the writer produced (one archive path for
/// archive writers; the loose-file list for `DirWriter`).
pub fn extract_with(
    layer: &LayerInfo,
    selector: &dyn FileSelector,
    mut writer: Box<dyn OutputWriter>,
) -> anyhow::Result<(usize, Vec<PathBuf>)> {
    let file = std::fs::File::open(&layer.blob_path)
        .with_context(|| format!("opening layer blob: {}", layer.blob_path.display()))?;

    let reader: Box<dyn Read> = if layer.media_type.contains("gzip")
        || layer.blob_path.extension().is_some_and(|e| e == "gz")
    {
        Box::new(GzDecoder::new(file))
    } else {
        Box::new(file)
    };

    let mut archive = tar::Archive::new(reader);
    let mut count: usize = 0;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.to_string_lossy().to_string();

        let Some(safe) = safe_path(&raw_path) else {
            continue;
        };
        if !selector.matches(&safe.absolute) {
            continue;
        }

        let header = entry.header().clone();
        writer.write_entry(&safe, &header, &mut entry)?;
        count += 1;
    }

    let outputs = writer.finish()?;
    Ok((count, outputs))
}

/// Build the appropriate [`OutputWriter`] for `format`. The `base_name`
/// is used to derive archive filenames; ignored for [`OutputFormat::Dir`]
/// which writes loose files directly under `output_dir`.
///
/// [`OutputFormat::Squashfs`] is not supported for selective extraction
/// (partial-selection squashfs needs external `mksquashfs`); callers
/// should fall back to a different format or use the whole-layer
/// `export_ocirender*` path.
pub fn writer_for_format(
    format: OutputFormat,
    output_dir: &Path,
    base_name: &str,
) -> anyhow::Result<Box<dyn OutputWriter>> {
    match format {
        OutputFormat::Dir => Ok(Box::new(DirWriter::new(output_dir)?)),
        OutputFormat::Tar => Ok(Box::new(TarWriter::new(
            output_dir.join(format!("{base_name}.tar")),
        )?)),
        OutputFormat::TarGz => Ok(Box::new(TarGzWriter::new(
            output_dir.join(format!("{base_name}.tar.gz")),
        )?)),
        OutputFormat::Squashfs => {
            anyhow::bail!("squashfs not yet supported for selective extraction")
        }
    }
}

/// Extract a selected set of paths from a layer's blob using the given
/// output format. Returns the entry count and the paths the writer
/// produced (one archive path for `Tar`/`TarGz`; the loose-file list
/// for `Dir`). Compatibility wrapper over [`extract_with`].
pub fn extract_files(
    layer: &LayerInfo,
    selected_paths: &[String],
    output_dir: &Path,
    format: OutputFormat,
    base_name: &str,
) -> anyhow::Result<(usize, Vec<PathBuf>)> {
    let selector = SelectedSet::new(selected_paths.iter().cloned());
    let writer = writer_for_format(format, output_dir, base_name)?;
    extract_with(layer, &selector, writer)
}

/// Export a layer as a tar.gz file in the output directory.
/// Returns the path to the created file.
pub fn export_layer(layer: &LayerInfo, output_dir: &Path) -> anyhow::Result<PathBuf> {
    let dest = output_dir.join(format!("layer-{}.tar.gz", layer.index));

    let blob_data = std::fs::read(&layer.blob_path)
        .with_context(|| format!("reading layer blob: {}", layer.blob_path.display()))?;

    // Check if already gzipped by looking for magic bytes
    if blob_data.len() >= 2 && blob_data[0] == 0x1f && blob_data[1] == 0x8b {
        std::fs::write(&dest, &blob_data)?;
    } else {
        let out = std::fs::File::create(&dest)?;
        let mut encoder = GzEncoder::new(out, Compression::default());
        std::io::copy(&mut std::io::Cursor::new(&blob_data), &mut encoder)?;
        encoder.finish()?;
    }

    Ok(dest)
}

/// Export all layers as tar.gz files. Returns paths created.
pub fn export_all_layers(layers: &[LayerInfo], output_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(output_dir)?;
    let mut paths = Vec::new();
    for layer in layers {
        paths.push(export_layer(layer, output_dir)?);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{LayerInfo, MEDIA_TYPE_LAYER_GZIP};
    use crate::tree::FileTree;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn write_temp(bytes: &[u8], suffix: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut path = std::env::temp_dir();
        let pid = std::process::id();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("imgchk-extract-test-{pid}-{seq}{suffix}"));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn tar_bytes(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, contents, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).unwrap();
            header.set_size(contents.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append(&header, *contents).unwrap();
        }
        builder.into_inner().unwrap()
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    fn layer_with_blob(blob_path: PathBuf, media_type: &str) -> LayerInfo {
        LayerInfo {
            index: 0,
            digest: "sha256:test".to_string(),
            diff_id: "sha256:test".to_string(),
            size: 0,
            command: String::new(),
            created: String::new(),
            file_tree: FileTree::new(),
            blob_path,
            media_type: media_type.to_string(),
        }
    }

    // ── OutputFormat ─────────────────────────────────────────────────────

    #[test]
    fn output_format_next_cycles_through_all_variants() {
        assert_eq!(OutputFormat::TarGz.next(), OutputFormat::Tar);
        assert_eq!(OutputFormat::Tar.next(), OutputFormat::Squashfs);
        assert_eq!(OutputFormat::Squashfs.next(), OutputFormat::Dir);
        assert_eq!(OutputFormat::Dir.next(), OutputFormat::TarGz);
    }

    #[test]
    fn output_format_label_matches_expected_strings() {
        assert_eq!(OutputFormat::TarGz.label(), "tar.gz");
        assert_eq!(OutputFormat::Tar.label(), "tar");
        assert_eq!(OutputFormat::Squashfs.label(), "squashfs");
        assert_eq!(OutputFormat::Dir.label(), "dir");
    }

    // ── make_image_spec ──────────────────────────────────────────────────

    #[test]
    fn make_image_spec_tar_joins_name_with_extension() {
        let spec = make_image_spec(OutputFormat::Tar, Path::new("/out"), "image");
        assert_eq!(spec.path(), Path::new("/out/image.tar"));
    }

    #[test]
    fn make_image_spec_squashfs_joins_name_with_extension() {
        let spec = make_image_spec(OutputFormat::Squashfs, Path::new("/out"), "image");
        assert_eq!(spec.path(), Path::new("/out/image.squashfs"));
    }

    #[test]
    fn make_image_spec_dir_joins_output_dir_with_bare_name() {
        let spec = make_image_spec(OutputFormat::Dir, Path::new("/out"), "image");
        assert_eq!(spec.path(), Path::new("/out/image"));
    }

    #[test]
    #[should_panic(expected = "TarGz uses the existing export path")]
    fn make_image_spec_targz_is_unreachable() {
        let _ = make_image_spec(OutputFormat::TarGz, Path::new("/out"), "image");
    }

    // ── writer_for_format ────────────────────────────────────────────────

    #[test]
    fn writer_for_format_dir_creates_output_directory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("nested");
        let _writer = writer_for_format(OutputFormat::Dir, &sub, "unused").unwrap();
        assert!(sub.is_dir());
    }

    #[test]
    fn writer_for_format_tar_creates_archive_file() {
        let dir = tempfile::tempdir().unwrap();
        let writer = writer_for_format(OutputFormat::Tar, dir.path(), "out").unwrap();
        let paths = writer.finish().unwrap();
        assert_eq!(paths, vec![dir.path().join("out.tar")]);
        assert!(paths[0].exists());
    }

    #[test]
    fn writer_for_format_targz_creates_archive_file() {
        let dir = tempfile::tempdir().unwrap();
        let writer = writer_for_format(OutputFormat::TarGz, dir.path(), "out").unwrap();
        let paths = writer.finish().unwrap();
        assert_eq!(paths, vec![dir.path().join("out.tar.gz")]);
        assert!(paths[0].exists());
    }

    #[test]
    fn writer_for_format_squashfs_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let result = writer_for_format(OutputFormat::Squashfs, dir.path(), "out");
        assert!(result.is_err());
    }

    // ── extract_with ─────────────────────────────────────────────────────

    #[test]
    fn extract_with_only_writes_selected_entries() {
        let tar = tar_bytes(&[
            ("etc/hosts", b"127.0.0.1 localhost\n", 0o644),
            ("etc/shadow", b"root:x:0:0\n", 0o600),
        ]);
        let blob_path = write_temp(&tar, ".tar");
        let layer = layer_with_blob(
            blob_path.clone(),
            "application/vnd.docker.image.rootfs.diff.tar",
        );

        let selector = SelectedSet::new(["/etc/hosts"]);
        let out_dir = tempfile::tempdir().unwrap();
        let writer = Box::new(DirWriter::new(out_dir.path()).unwrap());

        let (count, outputs) = extract_with(&layer, &selector, writer).unwrap();
        assert_eq!(count, 1);
        assert_eq!(outputs, vec![out_dir.path().join("etc/hosts")]);
        assert!(!out_dir.path().join("etc/shadow").exists());

        let _ = std::fs::remove_file(&blob_path);
    }

    #[test]
    fn extract_with_skips_entries_path_safety_rejects_even_if_selected() {
        // A raw tar entry path of ".." alone normalizes to nothing
        // (`safe_path` returns `None`), so it must be dropped before ever
        // reaching the selector — even a selector that would match it.
        // `Header::set_path` itself rejects `..` outright, so the raw name
        // bytes are poked directly to get such an entry into the archive.
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.as_gnu_mut().unwrap().name[..2].copy_from_slice(b"..");
        header.set_size(0);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &[][..]).unwrap();
        let tar_data = builder.into_inner().unwrap();

        let blob_path = write_temp(&tar_data, ".tar");
        let layer = layer_with_blob(
            blob_path.clone(),
            "application/vnd.docker.image.rootfs.diff.tar",
        );

        // A selector that matches everything — proves the skip happens at
        // the path-safety check, not the selector.
        struct MatchAll;
        impl FileSelector for MatchAll {
            fn matches(&self, _absolute_path: &str) -> bool {
                true
            }
        }

        let out_dir = tempfile::tempdir().unwrap();
        let writer = Box::new(DirWriter::new(out_dir.path()).unwrap());
        let (count, outputs) = extract_with(&layer, &MatchAll, writer).unwrap();

        assert_eq!(count, 0);
        assert!(outputs.is_empty());

        let _ = std::fs::remove_file(&blob_path);
    }

    #[test]
    fn extract_with_decompresses_gzip_via_media_type() {
        let tar = tar_bytes(&[("file.txt", b"hello", 0o644)]);
        // No ".gz"-shaped extension — only the media type signals gzip.
        let blob_path = write_temp(&gzip(&tar), ".bin");
        let layer = layer_with_blob(blob_path.clone(), MEDIA_TYPE_LAYER_GZIP);

        let selector = SelectedSet::new(["/file.txt"]);
        let out_dir = tempfile::tempdir().unwrap();
        let writer = Box::new(DirWriter::new(out_dir.path()).unwrap());
        let (count, _) = extract_with(&layer, &selector, writer).unwrap();

        assert_eq!(count, 1);
        assert_eq!(
            std::fs::read(out_dir.path().join("file.txt")).unwrap(),
            b"hello"
        );

        let _ = std::fs::remove_file(&blob_path);
    }

    #[test]
    fn extract_with_decompresses_gzip_via_extension_fallback() {
        let tar = tar_bytes(&[("file.txt", b"hello", 0o644)]);
        let blob_path = write_temp(&gzip(&tar), ".gz");
        // Media type deliberately doesn't mention gzip — only the ".gz"
        // extension should trigger decompression.
        let layer = layer_with_blob(blob_path.clone(), "application/octet-stream");

        let selector = SelectedSet::new(["/file.txt"]);
        let out_dir = tempfile::tempdir().unwrap();
        let writer = Box::new(DirWriter::new(out_dir.path()).unwrap());
        let (count, _) = extract_with(&layer, &selector, writer).unwrap();

        assert_eq!(count, 1);

        let _ = std::fs::remove_file(&blob_path);
    }

    // ── extract_files ────────────────────────────────────────────────────

    #[test]
    fn extract_files_end_to_end_with_dir_format() {
        let tar = tar_bytes(&[("a.txt", b"A", 0o644), ("b.txt", b"B", 0o644)]);
        let blob_path = write_temp(&tar, ".tar");
        let layer = layer_with_blob(
            blob_path.clone(),
            "application/vnd.docker.image.rootfs.diff.tar",
        );

        let out_dir = tempfile::tempdir().unwrap();
        let (count, outputs) = extract_files(
            &layer,
            &["/a.txt".to_string()],
            out_dir.path(),
            OutputFormat::Dir,
            "unused",
        )
        .unwrap();

        assert_eq!(count, 1);
        assert_eq!(outputs, vec![out_dir.path().join("a.txt")]);
        assert!(!out_dir.path().join("b.txt").exists());

        let _ = std::fs::remove_file(&blob_path);
    }

    // ── export_layer / export_all_layers ────────────────────────────────

    #[test]
    fn export_layer_copies_already_gzipped_blob_verbatim() {
        let tar = tar_bytes(&[("f", b"data", 0o644)]);
        let gz = gzip(&tar);
        let blob_path = write_temp(&gz, ".tar.gz");
        let layer = layer_with_blob(blob_path.clone(), MEDIA_TYPE_LAYER_GZIP);

        let out_dir = tempfile::tempdir().unwrap();
        let dest = export_layer(&layer, out_dir.path()).unwrap();

        // Verbatim copy, not re-encoded — byte-for-byte identical.
        assert_eq!(std::fs::read(&dest).unwrap(), gz);

        let _ = std::fs::remove_file(&blob_path);
    }

    #[test]
    fn export_layer_gzips_plain_tar_blob() {
        let tar = tar_bytes(&[("f", b"data", 0o644)]);
        let blob_path = write_temp(&tar, ".tar");
        let mut layer = layer_with_blob(
            blob_path.clone(),
            "application/vnd.docker.image.rootfs.diff.tar",
        );
        layer.index = 3;

        let out_dir = tempfile::tempdir().unwrap();
        let dest = export_layer(&layer, out_dir.path()).unwrap();
        assert_eq!(dest, out_dir.path().join("layer-3.tar.gz"));

        let mut decoder = GzDecoder::new(std::fs::File::open(&dest).unwrap());
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).unwrap();
        assert_eq!(decompressed, tar);

        let _ = std::fs::remove_file(&blob_path);
    }

    #[test]
    fn export_all_layers_writes_one_file_per_layer_in_order() {
        let tar0 = tar_bytes(&[("a", b"A", 0o644)]);
        let tar1 = tar_bytes(&[("b", b"B", 0o644)]);
        let blob0 = write_temp(&gzip(&tar0), ".tar.gz");
        let blob1 = write_temp(&gzip(&tar1), ".tar.gz");

        let mut layer0 = layer_with_blob(blob0.clone(), MEDIA_TYPE_LAYER_GZIP);
        layer0.index = 0;
        let mut layer1 = layer_with_blob(blob1.clone(), MEDIA_TYPE_LAYER_GZIP);
        layer1.index = 1;

        let out_dir = tempfile::tempdir().unwrap();
        let paths = export_all_layers(&[layer0, layer1], out_dir.path()).unwrap();

        assert_eq!(
            paths,
            vec![
                out_dir.path().join("layer-0.tar.gz"),
                out_dir.path().join("layer-1.tar.gz"),
            ]
        );
        assert!(paths.iter().all(|p| p.exists()));

        let _ = std::fs::remove_file(&blob0);
        let _ = std::fs::remove_file(&blob1);
    }

    // ── export_ocirender / export_ocirender_single ──────────────────────

    #[test]
    fn export_ocirender_dir_extracts_merged_filesystem() {
        let tar = tar_bytes(&[("usr/bin/hello", b"hi\n", 0o755)]);
        let blob_path = write_temp(&gzip(&tar), ".tar.gz");
        let layer = layer_with_blob(blob_path.clone(), MEDIA_TYPE_LAYER_GZIP);

        let out_dir = tempfile::tempdir().unwrap();
        let dest = out_dir.path().join("rootfs");
        export_ocirender(&[layer], ImageSpec::Dir { path: dest.clone() }).unwrap();

        assert_eq!(std::fs::read(dest.join("usr/bin/hello")).unwrap(), b"hi\n");

        let _ = std::fs::remove_file(&blob_path);
    }

    #[test]
    fn export_ocirender_single_re_indexes_to_zero() {
        let tar = tar_bytes(&[("etc/motd", b"welcome\n", 0o644)]);
        let blob_path = write_temp(&gzip(&tar), ".tar.gz");
        let mut layer = layer_with_blob(blob_path.clone(), MEDIA_TYPE_LAYER_GZIP);
        // Deliberately non-zero — export_ocirender_single always re-indexes
        // to 0 internally, so this must not affect the outcome.
        layer.index = 7;

        let out_dir = tempfile::tempdir().unwrap();
        let dest = out_dir.path().join("rootfs");
        export_ocirender_single(&layer, ImageSpec::Dir { path: dest.clone() }).unwrap();

        assert_eq!(std::fs::read(dest.join("etc/motd")).unwrap(), b"welcome\n");

        let _ = std::fs::remove_file(&blob_path);
    }
}
