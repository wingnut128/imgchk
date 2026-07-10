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
    // Consumed by Dockerfile reconstruction (upcoming task); not yet read.
    #[allow(dead_code)]
    pub history: Vec<HistoryStep>,
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

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct HistoryStep {
    pub created_by: String,
    pub empty_layer: bool,
    pub created: String,
}

pub(crate) fn parse_full_history(config: &ImageConfig) -> Vec<HistoryStep> {
    let mut steps = Vec::new();
    if let Some(history) = &config.history {
        for h in history {
            steps.push(HistoryStep {
                created_by: h.created_by.clone().unwrap_or_default(),
                empty_layer: h.empty_layer.unwrap_or(false),
                created: h.created.clone().unwrap_or_default(),
            });
        }
    }
    steps
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

    #[test]
    fn parse_full_history_keeps_empty_layers_and_order() {
        let config = ImageConfig {
            history: Some(vec![
                HistoryEntry {
                    created_by: Some("/bin/sh -c #(nop)  ENV A=1".into()),
                    created: Some("t0".into()),
                    empty_layer: Some(true),
                },
                HistoryEntry {
                    created_by: Some("/bin/sh -c apt-get update".into()),
                    created: Some("t1".into()),
                    empty_layer: Some(false),
                },
                HistoryEntry {
                    created_by: None,
                    created: None,
                    empty_layer: None,
                },
            ]),
            ..Default::default()
        };
        let steps = parse_full_history(&config);
        assert_eq!(steps.len(), 3);
        assert!(steps[0].empty_layer);
        assert_eq!(steps[0].created_by, "/bin/sh -c #(nop)  ENV A=1");
        assert!(!steps[1].empty_layer);
        assert_eq!(steps[1].created, "t1");
        // Missing fields default cleanly.
        assert_eq!(steps[2].created_by, "");
        assert!(!steps[2].empty_layer);
        assert_eq!(steps[2].created, "");
    }
}
