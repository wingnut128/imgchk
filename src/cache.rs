use std::path::{Path, PathBuf};

/// Default max cache size: 10 GB
const DEFAULT_MAX_CACHE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// Resolve the cache directory path.
pub fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("IMGCHK_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        super::image::home_dir()
            .map(|h| h.join(".cache").join("imgchk").join("blobs"))
            .unwrap_or_else(|| PathBuf::from(".cache/imgchk/blobs"))
    }
}

/// Max cache size from env or default.
fn max_cache_bytes() -> u64 {
    std::env::var("IMGCHK_CACHE_MAX_MB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|mb| mb * 1024 * 1024)
        .unwrap_or(DEFAULT_MAX_CACHE_BYTES)
}

/// Check whether there is enough free disk space for the given bytes.
/// Returns false if space cannot be determined or is insufficient.
pub fn has_disk_space(path: &Path, needed_bytes: u64) -> bool {
    available_space(path).is_some_and(|avail| avail > needed_bytes)
}

/// Get available disk space for the filesystem containing `path`.
fn available_space(path: &Path) -> Option<u64> {
    // Walk up to find an existing ancestor
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

    // Parse second line, 4th column (available KB)
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().nth(1)?;
    let avail_kb: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail_kb * 1024)
}

/// Evict oldest blobs from the cache until total size is under the limit.
/// Uses file access time (atime) for LRU ordering.
pub fn evict_if_needed(cache_dir: &Path) {
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

    // Sort by access time ascending (oldest first)
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
        // Skip .tmp files (in-progress writes)
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

/// Touch a cache file to update its access time (for LRU tracking).
pub fn touch(path: &Path) {
    let _ = std::fs::File::open(path);
}
