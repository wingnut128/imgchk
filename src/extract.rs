use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use ocirender::{ImageSpec, LayerMeta, StreamingPacker};

use crate::image::LayerInfo;
use crate::tree;

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

/// Extract specific files from a layer's blob to an output directory.
/// Returns the number of files extracted.
pub fn extract_files(
    layer: &LayerInfo,
    selected_paths: &[String],
    output_dir: &Path,
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

    let selected_set: std::collections::HashSet<&str> =
        selected_paths.iter().map(|s| s.as_str()).collect();

    std::fs::create_dir_all(output_dir)?;
    let canonical_out =
        std::fs::canonicalize(output_dir).unwrap_or_else(|_| output_dir.to_path_buf());

    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.to_string_lossy().to_string();
        let normalized = tree::normalize_path(&raw_path);

        if !selected_set.contains(normalized.as_str()) {
            continue;
        }

        let dest = output_dir.join(normalized.trim_start_matches('/'));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let canonical_dest = std::fs::canonicalize(dest.parent().unwrap_or(output_dir))
            .unwrap_or_else(|_| output_dir.to_path_buf())
            .join(dest.file_name().unwrap_or_default());
        if !canonical_dest.starts_with(&canonical_out) {
            continue;
        }

        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
        count += 1;
    }

    Ok(count)
}

/// Export a layer as a tar.gz file in the output directory.
/// Returns the path to the created file.
pub fn export_layer(layer: &LayerInfo, output_dir: &Path) -> anyhow::Result<PathBuf> {
    let dest = output_dir.join(format!("layer-{}.tar.gz", layer.index));

    let blob_data = std::fs::read(&layer.blob_path)
        .with_context(|| format!("reading layer blob: {}", layer.blob_path.display()))?;

    // Check if already gzipped by looking for magic bytes
    if blob_data.len() >= 2 && blob_data[0] == 0x1f && blob_data[1] == 0x8b {
        // Already gzipped, just copy
        std::fs::write(&dest, &blob_data)?;
    } else {
        // Compress as gzip
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
