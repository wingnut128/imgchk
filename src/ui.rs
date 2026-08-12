use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;

use crate::action::key_to_action;
use crate::extract::OutputFormat;
use crate::image::ImageInfo;
use crate::selection::Selection;
use crate::tree::{self, FileNode, FileTree};
use crate::update::update;
use crate::view;

/// RAII guard for terminal state. Ensures that raw mode is disabled and the
/// alternate screen is left on drop, even if the TUI loop exits via error.
struct TerminalGuard {
    cleaned_up: bool,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        Ok(TerminalGuard { cleaned_up: false })
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.cleaned_up {
            disable_raw_mode()?;
            io::stdout().execute(LeaveAlternateScreen)?;
            self.cleaned_up = true;
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Layers,
    Files,
    Details,
}

impl Pane {
    pub fn next(self) -> Self {
        match self {
            Pane::Layers => Pane::Files,
            Pane::Files => Pane::Details,
            Pane::Details => Pane::Layers,
        }
    }
}

pub struct TreeRow {
    pub depth: usize,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub expanded: bool,
    pub link_target: Option<String>,
}

/// Cursor positions, expansion state, and the cached file tree the view
/// renders. All navigation-related update arms touch only this struct.
pub struct NavState {
    pub layer_index: usize,
    pub file_index: usize,
    pub detail_scroll: u16,
    pub cumulative: bool,
    pub expanded_dirs: HashSet<String>,
    pub cached_tree: FileTree,
    pub file_rows: Vec<TreeRow>,
    /// Cache key: (layer_index, cumulative) for which cached_tree is valid.
    /// Avoids re-merging trees when only expanded_dirs or file_index change.
    cached_merge_key: Option<(usize, bool)>,
}

impl NavState {
    pub fn new() -> Self {
        Self {
            layer_index: 0,
            file_index: 0,
            detail_scroll: 0,
            cumulative: false,
            expanded_dirs: HashSet::new(),
            cached_tree: FileTree::new(),
            file_rows: Vec::new(),
            cached_merge_key: None,
        }
    }

    /// Rebuild [`Self::cached_tree`] and [`Self::file_rows`] from the
    /// current `layer_index` / `cumulative` state against the layers in
    /// `image`. Only recomputes the merged tree if the key (layer_index,
    /// cumulative) has changed; always re-flattens file_rows based on
    /// expanded_dirs. Clamps `file_index` if the new row count is shorter.
    pub fn rebuild_file_rows(&mut self, image: &ImageInfo) {
        let current_key = (self.layer_index, self.cumulative);

        // Only rebuild cached_tree if the key has changed.
        if self.cached_merge_key != Some(current_key) {
            self.cached_tree = if self.cumulative {
                let trees: Vec<FileTree> = image.layers[..=self.layer_index]
                    .iter()
                    .map(|l| l.file_tree.clone())
                    .collect();
                tree::merge_trees(&trees)
            } else if image.layers.is_empty() {
                FileTree::new()
            } else {
                image.layers[self.layer_index].file_tree.clone()
            };
            self.cached_merge_key = Some(current_key);
        }

        // Always re-flatten rows from the (possibly-unchanged) cached_tree.
        let mut rows = Vec::new();
        flatten_node(&self.cached_tree.root, 0, &self.expanded_dirs, &mut rows);
        self.file_rows = rows;
        if self.file_index >= self.file_rows.len() {
            self.file_index = self.file_rows.len().saturating_sub(1);
        }
    }
}

/// Output-directory choice, archive format, and the user-visible status
/// line. All extraction-result / format-cycle arms touch only this struct.
pub struct OutputState {
    pub dir: Option<PathBuf>,
    pub format: OutputFormat,
    pub status: String,
}

impl OutputState {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            format: OutputFormat::TarGz,
            status: String::new(),
        }
    }

    /// Return an output directory, lazily creating a tmpdir on first use
    /// and recording its path in `status` so the user can see where
    /// extractions land.
    ///
    /// On error (e.g., tmpdir creation failure), sets the status message and
    /// returns an Err containing a human-readable error string.
    pub fn ensure_dir(&mut self) -> Result<PathBuf, String> {
        if let Some(ref dir) = self.dir {
            let _ = std::fs::create_dir_all(dir);
            Ok(dir.clone())
        } else {
            match tempfile::tempdir() {
                Ok(tmp) => {
                    let path = tmp.keep();
                    self.status = format!("Output: {}", path.display());
                    self.dir = Some(path.clone());
                    Ok(path)
                }
                Err(e) => {
                    let msg = format!("Failed to create temp directory: {e}");
                    self.status = msg.clone();
                    Err(msg)
                }
            }
        }
    }
}

/// Modal text-input state for the "set output dir" prompt.
pub struct ModalState {
    pub active: bool,
    pub buffer: String,
}

impl ModalState {
    pub fn new() -> Self {
        Self {
            active: false,
            buffer: String::new(),
        }
    }
}

pub struct App {
    pub image: ImageInfo,
    pub focus: Pane,
    pub nav: NavState,
    pub selection: Selection,
    pub output: OutputState,
    pub modal: ModalState,
    pub rt_handle: tokio::runtime::Handle,
}

impl App {
    pub fn new(
        image: ImageInfo,
        output_dir: Option<PathBuf>,
        rt_handle: tokio::runtime::Handle,
    ) -> Self {
        let mut app = App {
            image,
            focus: Pane::Layers,
            nav: NavState::new(),
            selection: Selection::new(),
            output: OutputState::new(output_dir),
            modal: ModalState::new(),
            rt_handle,
        };
        app.rebuild_file_rows();
        app
    }

    pub fn rebuild_file_rows(&mut self) {
        self.nav.rebuild_file_rows(&self.image);
    }

    pub fn ensure_output_dir(&mut self) -> Result<PathBuf, String> {
        self.output.ensure_dir()
    }
}

fn flatten_node(
    node: &FileNode,
    depth: usize,
    expanded: &HashSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    let mut children: Vec<&FileNode> = node.children.values().collect();
    children.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));

    for child in children {
        let is_expanded = expanded.contains(&child.path);
        rows.push(TreeRow {
            depth,
            name: child.name.clone(),
            path: child.path.clone(),
            size: child.size,
            is_dir: child.is_dir,
            expanded: is_expanded,
            link_target: child.link_target.clone(),
        });

        if child.is_dir && is_expanded {
            flatten_node(child, depth + 1, expanded, rows);
        }
    }
}

pub fn run(
    image: ImageInfo,
    output_dir: Option<PathBuf>,
    rt_handle: tokio::runtime::Handle,
) -> anyhow::Result<()> {
    let mut guard = TerminalGuard::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(image, output_dir, rt_handle);

    loop {
        terminal.draw(|f| view::draw(f, &app))?;

        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = key_to_action(app.modal.active, app.focus, key.code)
            && update(&mut app, action).is_break()
        {
            break;
        }
    }

    guard.restore()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // TerminalGuard is deliberately untested: asserting that raw mode and the
    // alternate screen are restored needs a PTY, and merely constructing a
    // guard in a unit test would run disable_raw_mode() + LeaveAlternateScreen
    // against the terminal of whoever runs `cargo test`.

    #[test]
    fn output_state_ensure_dir_with_existing_dir_succeeds() {
        let dir = std::env::temp_dir();
        let mut state = OutputState::new(Some(dir.clone()));
        let result = state.ensure_dir();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dir);
    }

    #[test]
    fn output_state_ensure_dir_tmpdir_sets_status() {
        let mut state = OutputState::new(None);
        let dir = state.ensure_dir().expect("tmpdir creation should succeed");
        assert!(state.status.starts_with("Output:"));
        assert_eq!(state.dir.as_deref(), Some(dir.as_path()));
        // ensure_dir deliberately leaks its tmpdir so extractions outlive the
        // process, so the test has to remove it itself.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_state_ensure_dir_tmpdir_caches_result() {
        let mut state = OutputState::new(None);
        let first = state.ensure_dir().expect("first call should succeed");
        let second = state.ensure_dir().expect("second call should succeed");
        // The tmpdir is created once and reused, not recreated per call.
        assert_eq!(first, second);
        let _ = std::fs::remove_dir_all(&first);
    }
}
