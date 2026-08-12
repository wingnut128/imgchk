use std::path::{Path, PathBuf};

/// Default max cache size: 10 GB.
const DEFAULT_MAX_CACHE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

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
/// (or `$IMGCHK_CACHE_DIR`).
pub struct FsBlobStore {
    root: PathBuf,
}

impl FsBlobStore {
    pub fn new() -> Self {
        Self {
            root: default_cache_dir(),
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
        touch(&cache_path);
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
        let has_space = has_disk_space(&self.root, total_bytes);
        evict_if_needed(&self.root);
        has_space
    }
}

// ── Filesystem helpers (formerly src/cache.rs) ─────────────────────────────

fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("IMGCHK_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        super::home_dir()
            .map(|h| h.join(".cache").join("imgchk").join("blobs"))
            .unwrap_or_else(|| PathBuf::from(".cache/imgchk/blobs"))
    }
}

fn max_cache_bytes() -> u64 {
    std::env::var("IMGCHK_CACHE_MAX_MB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|mb| mb * 1024 * 1024)
        .unwrap_or(DEFAULT_MAX_CACHE_BYTES)
}

/// Whether the filesystem containing `path` reports more than `needed_bytes`
/// of free space. Returns `false` when space cannot be determined.
fn has_disk_space(path: &Path, needed_bytes: u64) -> bool {
    available_space(path).is_some_and(|avail| avail > needed_bytes)
}

fn available_space(path: &Path) -> Option<u64> {
    let mut check = path.to_path_buf();
    while !check.exists() {
        if !check.pop() {
            return None;
        }
    }

    let output = std::process::Command::new("df")
        .arg("-k")
        .arg(&check)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().nth(1)?;
    let avail_kb: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail_kb * 1024)
}

/// Evict oldest blobs (by access time) until the cache is under the
/// configured byte limit.
fn evict_if_needed(cache_dir: &Path) {
    let max_bytes = max_cache_bytes();
    if max_bytes == 0 {
        return;
    }

    let entries = match collect_cache_entries(cache_dir) {
        Some(e) => e,
        None => return,
    };

    let total: u64 = entries.iter().map(|e| e.size).sum();
    if total <= max_bytes {
        return;
    }

    let mut sorted = entries;
    sorted.sort_by_key(|e| e.accessed);

    let mut current = total;
    for entry in &sorted {
        if current <= max_bytes {
            break;
        }
        if std::fs::remove_file(&entry.path).is_ok() {
            current = current.saturating_sub(entry.size);
            eprintln!(
                "Cache evicted: {} ({})",
                entry.path.file_name().unwrap_or_default().to_string_lossy(),
                crate::tree::human_size(entry.size),
            );
        }
    }
}

struct CacheEntry {
    path: PathBuf,
    size: u64,
    accessed: std::time::SystemTime,
}

fn collect_cache_entries(cache_dir: &Path) -> Option<Vec<CacheEntry>> {
    let dir = std::fs::read_dir(cache_dir).ok()?;
    let mut entries = Vec::new();

    for entry in dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().is_some_and(|e| e == "tmp") {
            continue;
        }
        if let Ok(meta) = path.metadata() {
            entries.push(CacheEntry {
                path,
                size: meta.len(),
                accessed: meta.accessed().unwrap_or(std::time::UNIX_EPOCH),
            });
        }
    }

    Some(entries)
}

/// Bump a cache file's access time so LRU eviction sees it as recent.
/// Sets the access time explicitly using filetime, which works reliably across
/// different mount options (relatime, noatime, etc.). Best-effort: silently
/// ignores I/O errors.
fn touch(path: &Path) {
    let _ = filetime::set_file_atime(path, filetime::FileTime::now());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

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

    /// Serializes tests that mutate the process environment.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn default_cache_dir_uses_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("IMGCHK_CACHE_DIR", "/tmp/imgchk-test-override");
        }
        let dir = default_cache_dir();
        unsafe {
            std::env::remove_var("IMGCHK_CACHE_DIR");
        }
        assert_eq!(dir, PathBuf::from("/tmp/imgchk-test-override"));
    }

    #[test]
    fn max_cache_bytes_uses_default_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("IMGCHK_CACHE_MAX_MB");
        }
        assert_eq!(max_cache_bytes(), DEFAULT_MAX_CACHE_BYTES);
    }

    #[test]
    fn max_cache_bytes_respects_env_in_mb() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("IMGCHK_CACHE_MAX_MB", "5");
        }
        let bytes = max_cache_bytes();
        unsafe {
            std::env::remove_var("IMGCHK_CACHE_MAX_MB");
        }
        assert_eq!(bytes, 5 * 1024 * 1024);
    }

    #[test]
    fn ensure_capacity_does_not_panic_on_empty_dir() {
        let t = temp_store();
        // Empty dir — total is 0, well under any limit.
        let _ = t.store.ensure_capacity(1024);
    }

    #[test]
    fn touch_updates_access_time() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test_file");

        // Create the file with some content
        std::fs::write(&file_path, b"test").unwrap();

        // Set atime to a time clearly in the past (1 hour ago).
        let past = filetime::FileTime::from_system_time(
            SystemTime::now()
                .checked_sub(std::time::Duration::from_secs(3600))
                .unwrap(),
        );
        filetime::set_file_atime(&file_path, past).unwrap();

        // Record the past atime for comparison.
        let atime_before = std::fs::metadata(&file_path).unwrap().accessed().unwrap();

        // Brief pause to ensure time advances (some filesystems have coarse granularity).
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Call touch() to update the atime.
        touch(&file_path);

        // Verify that atime moved forward.
        let atime_after = std::fs::metadata(&file_path).unwrap().accessed().unwrap();
        assert!(
            atime_after > atime_before,
            "touch() must advance atime: before={:?}, after={:?}",
            atime_before,
            atime_after
        );
    }
}
