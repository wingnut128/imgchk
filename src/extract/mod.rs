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
/// written.
pub fn extract_with(
    layer: &LayerInfo,
    selector: &dyn FileSelector,
    mut writer: Box<dyn OutputWriter>,
) -> anyhow::Result<usize> {
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
    let mut count = 0;

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

    let _ = writer.finish()?;
    Ok(count)
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
/// output format. Compatibility wrapper over [`extract_with`].
pub fn extract_files(
    layer: &LayerInfo,
    selected_paths: &[String],
    output_dir: &Path,
    format: OutputFormat,
    base_name: &str,
) -> anyhow::Result<usize> {
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
