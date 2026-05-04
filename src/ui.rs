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

pub struct App {
    pub image: ImageInfo,
    pub focus: Pane,

    pub layer_index: usize,

    pub cumulative: bool,
    pub expanded_dirs: HashSet<String>,
    pub cached_tree: FileTree,
    pub file_rows: Vec<TreeRow>,
    pub file_index: usize,
    pub selection: Selection,

    pub output_dir: Option<PathBuf>,
    pub output_format: OutputFormat,

    pub detail_scroll: u16,

    pub status: String,

    pub input_mode: bool,
    pub input_buf: String,
}

impl App {
    pub fn new(image: ImageInfo, output_dir: Option<PathBuf>) -> Self {
        let mut app = App {
            image,
            focus: Pane::Layers,
            layer_index: 0,
            cumulative: false,
            expanded_dirs: HashSet::new(),
            cached_tree: FileTree::new(),
            file_rows: Vec::new(),
            file_index: 0,
            selection: Selection::new(),
            output_dir,
            output_format: OutputFormat::TarGz,
            detail_scroll: 0,
            status: String::new(),
            input_mode: false,
            input_buf: String::new(),
        };
        app.rebuild_file_rows();
        app
    }

    fn rebuild_tree(&mut self) {
        self.cached_tree = if self.cumulative {
            let trees: Vec<FileTree> = self.image.layers[..=self.layer_index]
                .iter()
                .map(|l| l.file_tree.clone())
                .collect();
            tree::merge_trees(&trees)
        } else if self.image.layers.is_empty() {
            FileTree::new()
        } else {
            self.image.layers[self.layer_index].file_tree.clone()
        };
    }

    pub fn rebuild_file_rows(&mut self) {
        self.rebuild_tree();
        let mut rows = Vec::new();
        flatten_node(&self.cached_tree.root, 0, &self.expanded_dirs, &mut rows);
        self.file_rows = rows;
        if self.file_index >= self.file_rows.len() {
            self.file_index = self.file_rows.len().saturating_sub(1);
        }
    }

    pub fn ensure_output_dir(&mut self) -> PathBuf {
        if let Some(ref dir) = self.output_dir {
            let _ = std::fs::create_dir_all(dir);
            dir.clone()
        } else {
            let tmp = tempfile::tempdir().expect("create tmpdir");
            let path = tmp.keep();
            self.status = format!("Output: {}", path.display());
            self.output_dir = Some(path.clone());
            path
        }
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
            && let Some(action) = key_to_action(app.input_mode, app.focus, key.code)
            && update(&mut app, action).is_break()
        {
            break;
        }
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
