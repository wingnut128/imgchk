use std::collections::{HashMap, HashSet};

use crate::tree::{FileNode, FileTree};

/// Aggregate selection state for a directory in the file tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirStatus {
    None,
    Partial,
    All,
}

/// File-pane selection state. Owns the set of selected file paths and a
/// cached per-directory aggregate refreshed against a `FileTree` whenever
/// the selection changes.
///
/// Invariant: the cache is consistent with `files` *as projected onto the
/// last `FileTree` passed to a mutating method*. Replace the tree (e.g.
/// when toggling cumulative view or changing layer) by calling `clear`
/// before re-using the `Selection`.
pub struct Selection {
    files: HashSet<String>,
    dir_status: HashMap<String, DirStatus>,
}

impl Selection {
    pub fn new() -> Self {
        Self {
            files: HashSet::new(),
            dir_status: HashMap::new(),
        }
    }

    pub fn paths(&self) -> &HashSet<String> {
        &self.files
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files.contains(path)
    }

    pub fn dir_status(&self, path: &str) -> DirStatus {
        self.dir_status
            .get(path)
            .copied()
            .unwrap_or(DirStatus::None)
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.dir_status.clear();
    }

    /// Toggle membership of a single file path.
    pub fn toggle_file(&mut self, path: &str, tree: &FileTree) {
        if self.files.contains(path) {
            self.files.remove(path);
        } else {
            self.files.insert(path.to_string());
        }
        self.refresh(tree);
    }

    /// If every file under `path` is selected, deselect them all; otherwise
    /// select them all. No-op if `path` does not resolve to a directory.
    pub fn toggle_under(&mut self, path: &str, tree: &FileTree) {
        let paths = tree.collect_under(path);
        if paths.is_empty() {
            return;
        }
        let all_selected = paths.iter().all(|p| self.files.contains(p));
        if all_selected {
            for p in &paths {
                self.files.remove(p);
            }
        } else {
            for p in paths {
                self.files.insert(p);
            }
        }
        self.refresh(tree);
    }

    fn refresh(&mut self, tree: &FileTree) {
        self.dir_status.clear();
        compute_dir_status(&tree.root, &self.files, &mut self.dir_status);
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::new()
    }
}

/// Walk the tree and populate `out` with a `DirStatus` for every directory.
/// Returns this node's status so parents can aggregate.
fn compute_dir_status(
    node: &FileNode,
    selected: &HashSet<String>,
    out: &mut HashMap<String, DirStatus>,
) -> DirStatus {
    if !node.is_dir {
        return if selected.contains(&node.path) {
            DirStatus::All
        } else {
            DirStatus::None
        };
    }

    if node.children.is_empty() {
        out.insert(node.path.clone(), DirStatus::None);
        return DirStatus::None;
    }

    let mut all_count = 0usize;
    let mut none_count = 0usize;
    let mut partial_seen = false;
    let total = node.children.len();

    for child in node.children.values() {
        match compute_dir_status(child, selected, out) {
            DirStatus::All => all_count += 1,
            DirStatus::None => none_count += 1,
            DirStatus::Partial => {
                partial_seen = true;
            }
        }
    }

    let status = if partial_seen {
        DirStatus::Partial
    } else if all_count == total {
        DirStatus::All
    } else if none_count == total {
        DirStatus::None
    } else {
        DirStatus::Partial
    };
    out.insert(node.path.clone(), status);
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{FileNode, FileTree};
    use std::collections::BTreeMap;

    fn file(path: &str) -> FileNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        FileNode {
            name,
            path: path.into(),
            size: 1,
            mode: 0o644,
            is_dir: false,
            is_whiteout: false,
            is_opaque: false,
            link_target: None,
            children: BTreeMap::new(),
        }
    }

    fn build_tree(paths: &[&str]) -> FileTree {
        let mut t = FileTree::new();
        for p in paths {
            t.insert_node(p, file(p));
            t.file_count += 1;
            t.total_size += 1;
        }
        t
    }

    #[test]
    fn new_is_empty() {
        let s = Selection::new();
        assert!(s.is_empty());
        assert_eq!(s.dir_status("/anywhere"), DirStatus::None);
    }

    #[test]
    fn toggle_file_flips_membership() {
        let t = build_tree(&["/a", "/b"]);
        let mut s = Selection::new();
        s.toggle_file("/a", &t);
        assert!(s.contains("/a"));
        assert!(!s.contains("/b"));
        s.toggle_file("/a", &t);
        assert!(!s.contains("/a"));
    }

    #[test]
    fn dir_status_all_when_every_file_selected() {
        let t = build_tree(&["/d/x", "/d/y"]);
        let mut s = Selection::new();
        s.toggle_file("/d/x", &t);
        s.toggle_file("/d/y", &t);
        assert_eq!(s.dir_status("/d"), DirStatus::All);
    }

    #[test]
    fn dir_status_partial_when_some_selected() {
        let t = build_tree(&["/d/x", "/d/y"]);
        let mut s = Selection::new();
        s.toggle_file("/d/x", &t);
        assert_eq!(s.dir_status("/d"), DirStatus::Partial);
    }

    #[test]
    fn dir_status_none_when_nothing_selected() {
        let _t = build_tree(&["/d/x", "/d/y"]);
        let s = Selection::new();
        // Status defaults to None even before any tree refresh has happened.
        assert_eq!(s.dir_status("/d"), DirStatus::None);
    }

    #[test]
    fn toggle_under_partial_selects_all() {
        let t = build_tree(&["/d/x", "/d/y"]);
        let mut s = Selection::new();
        s.toggle_file("/d/x", &t);
        s.toggle_under("/d", &t);
        assert!(s.contains("/d/x"));
        assert!(s.contains("/d/y"));
        assert_eq!(s.dir_status("/d"), DirStatus::All);
    }

    #[test]
    fn toggle_under_all_deselects_all() {
        let t = build_tree(&["/d/x", "/d/y"]);
        let mut s = Selection::new();
        s.toggle_under("/d", &t);
        assert_eq!(s.dir_status("/d"), DirStatus::All);
        s.toggle_under("/d", &t);
        assert!(s.is_empty());
        assert_eq!(s.dir_status("/d"), DirStatus::None);
    }

    #[test]
    fn nested_dirs_propagate_status() {
        // /root/a/* fully selected, /root/b/* untouched → /root is Partial.
        let t = build_tree(&["/root/a/x", "/root/a/y", "/root/b/z"]);
        let mut s = Selection::new();
        s.toggle_under("/root/a", &t);
        assert_eq!(s.dir_status("/root/a"), DirStatus::All);
        assert_eq!(s.dir_status("/root/b"), DirStatus::None);
        assert_eq!(s.dir_status("/root"), DirStatus::Partial);
    }

    #[test]
    fn clear_resets_files_and_cache() {
        let t = build_tree(&["/d/x", "/d/y"]);
        let mut s = Selection::new();
        s.toggle_under("/d", &t);
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.dir_status("/d"), DirStatus::None);
    }

    #[test]
    fn toggle_under_noop_for_file_path() {
        let t = build_tree(&["/a"]);
        let mut s = Selection::new();
        s.toggle_under("/a", &t);
        assert!(s.is_empty());
    }

    #[test]
    fn toggle_under_noop_for_missing_path() {
        let t = build_tree(&["/a"]);
        let mut s = Selection::new();
        s.toggle_under("/nope", &t);
        assert!(s.is_empty());
    }
}
