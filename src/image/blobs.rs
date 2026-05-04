use std::path::{Path, PathBuf};

/// Persistent storage for compressed image-layer blobs, keyed by digest.
///
/// Implementations are best-effort caches: callers must always be able to
/// fall back to re-downloading on a miss or write failure.
pub trait BlobStore: Send + Sync {
    /// Try to copy a cached blob into `dest`. Returns `true` on a verified
    /// cache hit (digest present, size matches). Updates LRU access time
    /// on hit.
    fn try_copy_cached(&self, digest: &str, expected_size: u64, dest: &Path) -> bool;

    /// Store `data` under `digest`. Best-effort: silently does nothing on
    /// I/O failure.
    fn store(&self, digest: &str, data: &[u8]);

    /// Hint that `total_bytes` are about to be downloaded. Triggers
    /// eviction if needed; returns `false` if the filesystem appears
    /// undersized (caller decides whether to warn or proceed).
    fn ensure_capacity(&self, total_bytes: u64) -> bool;
}

/// Filesystem-backed blob store rooted under `~/.cache/imgchk/blobs/`
/// (or `IMGCHK_CACHE_DIR`).
pub struct FsBlobStore {
    root: PathBuf,
}

impl FsBlobStore {
    pub fn new() -> Self {
        Self {
            root: crate::cache::cache_dir(),
        }
    }
}

impl Default for FsBlobStore {
    fn default() -> Self {
        Self::new()
    }
}

fn cache_path(root: &Path, digest: &str) -> PathBuf {
    // "sha256:abcdef..." -> "sha256-abcdef..."
    root.join(digest.replace(':', "-"))
}

impl BlobStore for FsBlobStore {
    fn try_copy_cached(&self, digest: &str, expected_size: u64, dest: &Path) -> bool {
        if expected_size == 0 {
            return false;
        }
        let cache_path = cache_path(&self.root, digest);
        let hit = cache_path
            .metadata()
            .ok()
            .is_some_and(|m| m.len() == expected_size);
        if !hit {
            return false;
        }
        crate::cache::touch(&cache_path);
        std::fs::copy(&cache_path, dest).is_ok()
    }

    fn store(&self, digest: &str, data: &[u8]) {
        if std::fs::create_dir_all(&self.root).is_err() {
            return;
        }
        let final_path = cache_path(&self.root, digest);
        let tmp_path = self.root.join(format!("{}.tmp", digest.replace(':', "-")));
        if std::fs::write(&tmp_path, data).is_ok() {
            let _ = std::fs::rename(&tmp_path, &final_path);
        }
    }

    fn ensure_capacity(&self, total_bytes: u64) -> bool {
        let has_space = crate::cache::has_disk_space(&self.root, total_bytes);
        crate::cache::evict_if_needed(&self.root);
        has_space
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempStore {
        _dir: tempfile::TempDir,
        store: FsBlobStore,
    }

    fn temp_store() -> TempStore {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore {
            root: dir.path().to_path_buf(),
        };
        TempStore { _dir: dir, store }
    }

    #[test]
    fn miss_when_blob_not_cached() {
        let t = temp_store();
        let dest = t._dir.path().join("dest");
        assert!(!t.store.try_copy_cached("sha256:deadbeef", 4, &dest));
        assert!(!dest.exists());
    }

    #[test]
    fn store_then_hit_round_trip() {
        let t = temp_store();
        let dest = t._dir.path().join("dest");
        t.store.store("sha256:abc", b"hello");
        assert!(t.store.try_copy_cached("sha256:abc", 5, &dest));
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
    }

    #[test]
    fn miss_on_size_mismatch() {
        let t = temp_store();
        let dest = t._dir.path().join("dest");
        t.store.store("sha256:abc", b"hello");
        // Wrong expected size — treat as miss (corrupt entry).
        assert!(!t.store.try_copy_cached("sha256:abc", 99, &dest));
    }

    #[test]
    fn miss_on_zero_expected_size() {
        let t = temp_store();
        let dest = t._dir.path().join("dest");
        t.store.store("sha256:empty", b"");
        // Zero-sized entries are rejected to avoid false hits on missing files.
        assert!(!t.store.try_copy_cached("sha256:empty", 0, &dest));
    }
}
