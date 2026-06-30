use std::collections::BTreeMap;
use std::io::Read;

/// A node in the layer file tree.
#[derive(Clone, Debug)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub mode: u32,
    pub is_dir: bool,
    pub is_whiteout: bool,
    pub is_opaque: bool,
    /// True for character/block device and FIFO entries (e.g. /dev/null).
    /// These carry permission bits governed by device semantics, not
    /// regular-file content access, so they're excluded from
    /// content-oriented checks like the suspicious-file scan.
    pub is_special: bool,
    pub link_target: Option<String>,
    pub children: BTreeMap<String, FileNode>,
}

/// Root of a layer's filesystem tree.
#[derive(Clone, Debug)]
pub struct FileTree {
    pub root: FileNode,
    pub file_count: usize,
    pub total_size: u64,
}

impl FileTree {
    pub fn new() -> Self {
        Self {
            root: FileNode {
                name: "/".into(),
                path: "/".into(),
                size: 0,
                mode: 0o755,
                is_dir: true,
                is_whiteout: false,
                is_opaque: false,
                is_special: false,
                link_target: None,
                children: BTreeMap::new(),
            },
            file_count: 0,
            total_size: 0,
        }
    }

    /// Build a file tree by reading tar headers from a reader.
    pub fn from_tar<R: Read>(reader: R) -> anyhow::Result<Self> {
        let mut tree = Self::new();
        let mut archive = tar::Archive::new(reader);

        for entry in archive.entries()? {
            let entry = entry?;
            let header = entry.header();

            let raw_path = entry.path()?.to_string_lossy().to_string();
            let path = normalize_path(&raw_path);
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();

            if name.is_empty() || path == "/" {
                continue;
            }

            let is_whiteout = name.starts_with(".wh.");
            let is_opaque = name == ".wh..wh..opq";
            let is_dir = header.entry_type() == tar::EntryType::Directory;
            let is_special = matches!(
                header.entry_type(),
                tar::EntryType::Char | tar::EntryType::Block | tar::EntryType::Fifo
            );
            let link_target = match header.entry_type() {
                tar::EntryType::Symlink | tar::EntryType::Link => {
                    header.link_name()?.map(|p| p.to_string_lossy().to_string())
                }
                _ => None,
            };

            let node = FileNode {
                name: name.clone(),
                path: path.clone(),
                size: header.size().unwrap_or(0),
                mode: header.mode().unwrap_or(0),
                is_dir,
                is_whiteout,
                is_opaque,
                is_special,
                link_target,
                children: BTreeMap::new(),
            };

            if !is_dir {
                tree.file_count += 1;
                tree.total_size += node.size;
            }

            tree.insert_node(&path, node);
        }

        Ok(tree)
    }

    pub(crate) fn insert_node(&mut self, path: &str, node: FileNode) {
        let parts: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if parts.is_empty() {
            return;
        }

        let mut current = &mut self.root;

        // Ensure parent directories exist
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Insert the actual node
                if node.is_dir {
                    current
                        .children
                        .entry(part.to_string())
                        .and_modify(|existing| {
                            existing.mode = node.mode;
                        })
                        .or_insert(node.clone());
                } else {
                    current.children.insert(part.to_string(), node.clone());
                }
            } else {
                // Ensure intermediate directory exists
                let dir_path = format!("/{}", parts[..=i].join("/"));
                current = current
                    .children
                    .entry(part.to_string())
                    .or_insert_with(|| FileNode {
                        name: part.to_string(),
                        path: dir_path,
                        size: 0,
                        mode: 0o755,
                        is_dir: true,
                        is_whiteout: false,
                        is_opaque: false,
                        is_special: false,
                        link_target: None,
                        children: BTreeMap::new(),
                    });
            }
        }
    }
}

/// Merge multiple layer trees into a cumulative view with whiteout handling.
pub fn merge_trees(trees: &[FileTree]) -> FileTree {
    let mut merged = FileTree::new();
    for tree in trees {
        apply_layer(&mut merged.root, &tree.root);
    }
    recount(&mut merged);
    merged
}

fn apply_layer(target: &mut FileNode, source: &FileNode) {
    for (name, src_child) in &source.children {
        if src_child.is_opaque {
            target.children.clear();
            continue;
        }

        if src_child.is_whiteout {
            let delete_name = name.strip_prefix(".wh.").unwrap_or(name);
            target.children.remove(delete_name);
            continue;
        }

        if src_child.is_dir {
            let existing = target
                .children
                .entry(name.clone())
                .or_insert_with(|| FileNode {
                    name: src_child.name.clone(),
                    path: src_child.path.clone(),
                    size: 0,
                    mode: src_child.mode,
                    is_dir: true,
                    is_whiteout: false,
                    is_opaque: false,
                    is_special: false,
                    link_target: None,
                    children: BTreeMap::new(),
                });
            if !existing.is_dir {
                *existing = FileNode {
                    name: src_child.name.clone(),
                    path: src_child.path.clone(),
                    size: 0,
                    mode: src_child.mode,
                    is_dir: true,
                    is_whiteout: false,
                    is_opaque: false,
                    is_special: false,
                    link_target: None,
                    children: BTreeMap::new(),
                };
            }
            apply_layer(existing, src_child);
        } else {
            target.children.insert(
                name.clone(),
                FileNode {
                    name: src_child.name.clone(),
                    path: src_child.path.clone(),
                    size: src_child.size,
                    mode: src_child.mode,
                    is_dir: false,
                    is_whiteout: false,
                    is_opaque: false,
                    is_special: src_child.is_special,
                    link_target: src_child.link_target.clone(),
                    children: BTreeMap::new(),
                },
            );
        }
    }
}

fn recount(tree: &mut FileTree) {
    tree.file_count = 0;
    tree.total_size = 0;
    count_node(&tree.root, &mut tree.file_count, &mut tree.total_size);
}

fn count_node(node: &FileNode, count: &mut usize, total: &mut u64) {
    for child in node.children.values() {
        if child.is_dir {
            count_node(child, count, total);
        } else {
            *count += 1;
            *total += child.size;
        }
    }
}

impl FileTree {
    /// Find a node by absolute path (e.g. `/usr/bin/sh`). Root `/` returns the root.
    pub fn find(&self, path: &str) -> Option<&FileNode> {
        if path == "/" {
            return Some(&self.root);
        }
        let parts = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty());
        let mut current = &self.root;
        for part in parts {
            current = current.children.get(part)?;
        }
        Some(current)
    }

    /// All file paths under directory `path`, recursively. Empty if `path`
    /// does not resolve to a directory.
    pub fn collect_under(&self, path: &str) -> Vec<String> {
        match self.find(path) {
            Some(node) if node.is_dir => collect_paths(node),
            _ => Vec::new(),
        }
    }

    /// All file paths in the tree.
    pub fn all_paths(&self) -> Vec<String> {
        collect_paths(&self.root)
    }
}

/// Collect all file paths under a node recursively.
pub(crate) fn collect_paths(node: &FileNode) -> Vec<String> {
    let mut paths = Vec::new();
    collect_paths_inner(node, &mut paths);
    paths
}

fn collect_paths_inner(node: &FileNode, paths: &mut Vec<String>) {
    if !node.is_dir {
        paths.push(node.path.clone());
        return;
    }
    for child in node.children.values() {
        collect_paths_inner(child, paths);
    }
}

/// Normalize a tar path to absolute form: /foo/bar
/// Rejects path traversal components (..) to prevent writes outside output dirs.
pub fn normalize_path(raw: &str) -> String {
    let stripped = raw.trim_start_matches("./").trim_start_matches('/');
    let parts: Vec<&str> = stripped
        .split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .collect();
    if parts.is_empty() {
        "/".into()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Format bytes as human-readable size.
pub fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    match bytes {
        b if b >= GB => format!("{:.1} GB", b as f64 / GB as f64),
        b if b >= MB => format!("{:.1} MB", b as f64 / MB as f64),
        b if b >= KB => format!("{:.1} KB", b as f64 / KB as f64),
        b => format!("{b} B"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tar_with_entry_type(entry_type: tar::EntryType) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_path("dev/null").unwrap();
        header.set_entry_type(entry_type);
        header.set_size(0);
        header.set_mode(0o666);
        header.set_cksum();
        builder.append(&header, &[][..]).unwrap();
        builder.into_inner().unwrap()
    }

    #[test]
    fn from_tar_marks_char_device_as_special() {
        let data = write_tar_with_entry_type(tar::EntryType::Char);
        let tree = FileTree::from_tar(std::io::Cursor::new(data)).unwrap();
        let node = tree.find("/dev/null").unwrap();
        assert!(node.is_special);
    }

    #[test]
    fn from_tar_marks_block_device_as_special() {
        let data = write_tar_with_entry_type(tar::EntryType::Block);
        let tree = FileTree::from_tar(std::io::Cursor::new(data)).unwrap();
        let node = tree.find("/dev/null").unwrap();
        assert!(node.is_special);
    }

    #[test]
    fn from_tar_marks_fifo_as_special() {
        let data = write_tar_with_entry_type(tar::EntryType::Fifo);
        let tree = FileTree::from_tar(std::io::Cursor::new(data)).unwrap();
        let node = tree.find("/dev/null").unwrap();
        assert!(node.is_special);
    }

    #[test]
    fn from_tar_regular_file_is_not_special() {
        let mut builder = tar::Builder::new(Vec::new());
        let payload = b"hi";
        let mut header = tar::Header::new_gnu();
        header.set_path("etc/foo").unwrap();
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &payload[..]).unwrap();
        let data = builder.into_inner().unwrap();

        let tree = FileTree::from_tar(std::io::Cursor::new(data)).unwrap();
        let node = tree.find("/etc/foo").unwrap();
        assert!(!node.is_special);
    }

    fn file(path: &str, size: u64) -> FileNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        FileNode {
            name,
            path: path.into(),
            size,
            mode: 0o644,
            is_dir: false,
            is_whiteout: false,
            is_opaque: false,
            is_special: false,
            link_target: None,
            children: BTreeMap::new(),
        }
    }

    fn dir(path: &str) -> FileNode {
        let name = if path == "/" {
            "/".into()
        } else {
            path.rsplit('/').next().unwrap_or(path).to_string()
        };
        FileNode {
            name,
            path: path.into(),
            size: 0,
            mode: 0o755,
            is_dir: true,
            is_whiteout: false,
            is_opaque: false,
            is_special: false,
            link_target: None,
            children: BTreeMap::new(),
        }
    }

    fn whiteout(parent_path: &str, target: &str) -> FileNode {
        let name = format!(".wh.{target}");
        FileNode {
            name: name.clone(),
            path: format!("{}/{}", parent_path.trim_end_matches('/'), name),
            size: 0,
            mode: 0o644,
            is_dir: false,
            is_whiteout: true,
            is_opaque: false,
            is_special: false,
            link_target: None,
            children: BTreeMap::new(),
        }
    }

    fn opaque(parent_path: &str) -> FileNode {
        let name = ".wh..wh..opq".to_string();
        FileNode {
            name: name.clone(),
            path: format!("{}/{}", parent_path.trim_end_matches('/'), name),
            size: 0,
            mode: 0o644,
            is_dir: false,
            is_whiteout: true,
            is_opaque: true,
            is_special: false,
            link_target: None,
            children: BTreeMap::new(),
        }
    }

    /// Build a tree from a list of (path, is_dir) pairs. Parent dirs are auto-created.
    fn build_tree(entries: &[(&str, bool)]) -> FileTree {
        let mut t = FileTree::new();
        for (path, is_dir) in entries {
            let node = if *is_dir { dir(path) } else { file(path, 1) };
            if !is_dir {
                t.file_count += 1;
                t.total_size += 1;
            }
            t.insert_node(path, node);
        }
        t
    }

    // ── normalize_path ─────────────────────────────────────────────────────

    #[test]
    fn normalize_strips_leading_dot_slash() {
        assert_eq!(normalize_path("./foo"), "/foo");
    }

    #[test]
    fn normalize_strips_dotdot() {
        // `..` components are dropped entirely (not popped) — keeps any path
        // anchored under the output dir even if a tarball tries to escape.
        assert_eq!(normalize_path("foo/../bar"), "/foo/bar");
    }

    #[test]
    fn normalize_strips_single_dot() {
        assert_eq!(normalize_path("foo/./bar"), "/foo/bar");
    }

    #[test]
    fn normalize_root_stays_root() {
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn normalize_empty_becomes_root() {
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn normalize_collapses_double_slashes() {
        assert_eq!(normalize_path("foo//bar"), "/foo/bar");
    }

    #[test]
    fn normalize_rejects_traversal_attempt() {
        // `..` components must be dropped so writes stay within output dirs.
        assert_eq!(normalize_path("../etc/passwd"), "/etc/passwd");
        assert_eq!(normalize_path("/foo/../../etc"), "/foo/etc");
    }

    // ── FileTree::find / collect_under / all_paths ────────────────────────

    #[test]
    fn find_returns_root() {
        let t = build_tree(&[("/usr/bin/sh", false)]);
        let n = t.find("/").unwrap();
        assert!(n.is_dir);
        assert_eq!(n.path, "/");
    }

    #[test]
    fn find_returns_existing_file() {
        let t = build_tree(&[("/usr/bin/sh", false), ("/etc/hosts", false)]);
        let n = t.find("/etc/hosts").unwrap();
        assert!(!n.is_dir);
        assert_eq!(n.path, "/etc/hosts");
    }

    #[test]
    fn find_returns_existing_dir() {
        let t = build_tree(&[("/usr/bin/sh", false)]);
        let n = t.find("/usr/bin").unwrap();
        assert!(n.is_dir);
    }

    #[test]
    fn find_returns_none_for_missing() {
        let t = build_tree(&[("/usr/bin/sh", false)]);
        assert!(t.find("/etc/missing").is_none());
        assert!(t.find("/usr/bin/sh/extra").is_none());
    }

    #[test]
    fn collect_under_returns_subtree_files() {
        let t = build_tree(&[
            ("/usr/bin/sh", false),
            ("/usr/bin/ls", false),
            ("/etc/hosts", false),
        ]);
        let mut paths = t.collect_under("/usr");
        paths.sort();
        assert_eq!(paths, vec!["/usr/bin/ls", "/usr/bin/sh"]);
    }

    #[test]
    fn collect_under_empty_for_file_path() {
        let t = build_tree(&[("/usr/bin/sh", false)]);
        assert!(t.collect_under("/usr/bin/sh").is_empty());
    }

    #[test]
    fn collect_under_empty_for_missing_path() {
        let t = build_tree(&[("/usr/bin/sh", false)]);
        assert!(t.collect_under("/nope").is_empty());
    }

    #[test]
    fn all_paths_returns_every_file() {
        let t = build_tree(&[("/a", false), ("/b/c", false), ("/b/d", false)]);
        let mut paths = t.all_paths();
        paths.sort();
        assert_eq!(paths, vec!["/a", "/b/c", "/b/d"]);
    }

    // ── merge_trees ────────────────────────────────────────────────────────

    #[test]
    fn merge_two_disjoint_layers_unions() {
        let a = build_tree(&[("/a", false)]);
        let b = build_tree(&[("/b", false)]);
        let m = merge_trees(&[a, b]);
        let mut paths = m.all_paths();
        paths.sort();
        assert_eq!(paths, vec!["/a", "/b"]);
        assert_eq!(m.file_count, 2);
    }

    #[test]
    fn merge_later_layer_overwrites_file() {
        let a = build_tree(&[("/x", false)]);
        let mut b = FileTree::new();
        let mut newer = file("/x", 99);
        newer.mode = 0o600;
        b.insert_node("/x", newer);
        b.file_count = 1;
        b.total_size = 99;
        let m = merge_trees(&[a, b]);
        let n = m.find("/x").unwrap();
        assert_eq!(n.size, 99);
        assert_eq!(n.mode, 0o600);
    }

    #[test]
    fn merge_whiteout_removes_file() {
        let a = build_tree(&[("/foo", false), ("/bar", false)]);
        let mut b = FileTree::new();
        b.insert_node("/.wh.foo", whiteout("/", "foo"));
        let m = merge_trees(&[a, b]);
        assert!(m.find("/foo").is_none());
        assert!(m.find("/bar").is_some());
    }

    #[test]
    fn merge_whiteout_of_directory_removes_subtree() {
        let a = build_tree(&[("/data/a", false), ("/data/b", false), ("/keep", false)]);
        let mut b = FileTree::new();
        b.insert_node("/.wh.data", whiteout("/", "data"));
        let m = merge_trees(&[a, b]);
        assert!(m.find("/data").is_none());
        assert!(m.find("/data/a").is_none());
        assert!(m.find("/keep").is_some());
    }

    #[test]
    fn merge_opaque_dir_clears_children() {
        let a = build_tree(&[("/data/old", false)]);
        let mut b = FileTree::new();
        b.insert_node("/data", dir("/data"));
        b.insert_node("/data/.wh..wh..opq", opaque("/data"));
        b.insert_node("/data/new", file("/data/new", 1));
        b.file_count = 1;
        b.total_size = 1;
        let m = merge_trees(&[a, b]);
        assert!(m.find("/data/old").is_none());
        assert!(m.find("/data/new").is_some());
    }
}
