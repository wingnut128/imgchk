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
    pub fn ensure_dir(&mut self) -> PathBuf {
        if let Some(ref dir) = self.dir {
            let _ = std::fs::create_dir_all(dir);
            dir.clone()
        } else {
            let tmp = tempfile::tempdir().expect("create tmpdir");
            let path = tmp.keep();
            self.status = format!("Output: {}", path.display());
            self.dir = Some(path.clone());
            path
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
}

impl App {
    pub fn new(image: ImageInfo, output_dir: Option<PathBuf>) -> Self {
        let mut app = App {
            image,
            focus: Pane::Layers,
            nav: NavState::new(),
            selection: Selection::new(),
            output: OutputState::new(output_dir),
            modal: ModalState::new(),
        };
        app.rebuild_file_rows();
        app
    }

    pub fn rebuild_file_rows(&mut self) {
        self.nav.rebuild_file_rows(&self.image);
    }

    pub fn ensure_output_dir(&mut self) -> PathBuf {
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

pub fn run(image: ImageInfo, output_dir: Option<PathBuf>) -> anyhow::Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(image, output_dir);

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

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
