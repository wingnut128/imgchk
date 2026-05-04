use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::tree::FileTree;

mod blobs;
mod creds;
mod registry;
mod tarball;

pub use blobs::{BlobStore, FsBlobStore};
pub use creds::{CredentialResolver, DefaultCredentials};
pub use registry::RegistrySource;
pub use tarball::TarballSource;

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

/// A source that can produce an [`ImageInfo`] from a string reference
/// (image name, tarball path, etc.).
///
/// Implementors: [`TarballSource`], [`RegistrySource`].
#[allow(async_fn_in_trait)]
pub trait ImageSource {
    async fn load(&self, reference: &str, platform: Option<&str>) -> anyhow::Result<ImageInfo>;
}

/// Detect whether a source string points at a local tarball file.
pub fn is_tarball(source: &str) -> bool {
    let path = Path::new(source);
    path.exists() && path.is_file()
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

// ── Shared image-config deserializers ──────────────────────────────────────

#[derive(Deserialize, Default)]
pub(crate) struct ImageConfig {
    pub(crate) architecture: Option<String>,
    pub(crate) os: Option<String>,
    pub(crate) history: Option<Vec<HistoryEntry>>,
    pub(crate) rootfs: Option<RootFs>,
}

#[derive(Deserialize)]
pub(crate) struct HistoryEntry {
    pub(crate) created_by: Option<String>,
    pub(crate) created: Option<String>,
    pub(crate) empty_layer: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct RootFs {
    pub(crate) diff_ids: Option<Vec<String>>,
}

pub(crate) fn parse_history(config: &ImageConfig) -> (Vec<String>, Vec<String>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn is_tarball_true_for_existing_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello").unwrap();
        assert!(is_tarball(f.path().to_str().unwrap()));
    }

    #[test]
    fn is_tarball_false_for_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_tarball(dir.path().to_str().unwrap()));
    }

    #[test]
    fn is_tarball_false_for_nonexistent() {
        assert!(!is_tarball("/this/does/not/exist/imgchk-test"));
    }

    #[test]
    fn is_tarball_false_for_registry_ref() {
        assert!(!is_tarball("nginx:latest"));
        assert!(!is_tarball("ghcr.io/org/app:v1.2"));
    }
}
