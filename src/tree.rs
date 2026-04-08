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
                link_target,
                children: if is_dir {
                    BTreeMap::new()
                } else {
                    BTreeMap::new()
                },
            };

            if !is_dir {
                tree.file_count += 1;
                tree.total_size += node.size;
            }

            tree.insert_node(&path, node);
        }

        Ok(tree)
    }

    fn insert_node(&mut self, path: &str, node: FileNode) {
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
                let dir_path =
                    format!("/{}", parts[..=i].join("/"));
                current =
                    current
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

/// Collect all file paths under a node recursively.
pub fn collect_paths(node: &FileNode) -> Vec<String> {
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
fn normalize_path(raw: &str) -> String {
    let stripped = raw.trim_start_matches("./").trim_start_matches('/');
    if stripped.is_empty() {
        "/".into()
    } else {
        format!("/{}", stripped.trim_end_matches('/'))
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
        b => format!("{} B", b),
    }
}
