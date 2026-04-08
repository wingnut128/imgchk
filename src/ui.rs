use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::extract;
use crate::image::ImageInfo;
use crate::tree::{self, FileNode, FileTree};

// ── Pane focus ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Layers,
    Files,
    Details,
}

impl Pane {
    fn next(self) -> Self {
        match self {
            Pane::Layers => Pane::Files,
            Pane::Files => Pane::Details,
            Pane::Details => Pane::Layers,
        }
    }
}

// ── Flattened tree row ──────────────────────────────────────────────────────

struct TreeRow {
    depth: usize,
    name: String,
    path: String,
    size: u64,
    is_dir: bool,
    expanded: bool,
    mode: u32,
    link_target: Option<String>,
}

// ── App state ───────────────────────────────────────────────────────────────

pub struct App {
    image: ImageInfo,
    focus: Pane,

    // Layer list
    layer_index: usize,

    // File tree
    cumulative: bool,
    expanded_dirs: HashSet<String>,
    file_rows: Vec<TreeRow>,
    file_index: usize,
    selected_files: HashSet<String>,

    // Output
    output_dir: Option<PathBuf>,

    // Status bar message
    status: String,

    // Set output mode (typing in status bar)
    input_mode: bool,
    input_buf: String,
}

impl App {
    pub fn new(image: ImageInfo, output_dir: Option<PathBuf>) -> Self {
        let mut app = App {
            image,
            focus: Pane::Layers,
            layer_index: 0,
            cumulative: false,
            expanded_dirs: HashSet::new(),
            file_rows: Vec::new(),
            file_index: 0,
            selected_files: HashSet::new(),
            output_dir,
            status: String::new(),
            input_mode: false,
            input_buf: String::new(),
        };
        app.rebuild_file_rows();
        app
    }

    fn current_tree(&self) -> FileTree {
        if self.cumulative {
            let trees: Vec<FileTree> = self.image.layers[..=self.layer_index]
                .iter()
                .map(|l| l.file_tree.clone())
                .collect();
            tree::merge_trees(&trees)
        } else {
            self.image.layers[self.layer_index].file_tree.clone()
        }
    }

    fn rebuild_file_rows(&mut self) {
        let tree = self.current_tree();
        let mut rows = Vec::new();
        flatten_node(&tree.root, 0, &self.expanded_dirs, &mut rows);
        self.file_rows = rows;
        if self.file_index >= self.file_rows.len() {
            self.file_index = self.file_rows.len().saturating_sub(1);
        }
    }

    fn ensure_output_dir(&mut self) -> PathBuf {
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

fn flatten_node(node: &FileNode, depth: usize, expanded: &HashSet<String>, rows: &mut Vec<TreeRow>) {
    // Sort: dirs first, then by name
    let mut children: Vec<&FileNode> = node.children.values().collect();
    children.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name))
    });

    for child in children {
        let is_expanded = expanded.contains(&child.path);
        rows.push(TreeRow {
            depth,
            name: child.name.clone(),
            path: child.path.clone(),
            size: child.size,
            is_dir: child.is_dir,
            expanded: is_expanded,
            mode: child.mode,
            link_target: child.link_target.clone(),
        });

        if child.is_dir && is_expanded {
            flatten_node(child, depth + 1, expanded, rows);
        }
    }
}

// ── Run loop ────────────────────────────────────────────────────────────────

pub fn run(image: ImageInfo, output_dir: Option<PathBuf>) -> anyhow::Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(image, output_dir);

    loop {
        terminal.draw(|f| draw(f, &app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Input mode for setting output dir
            if app.input_mode {
                match key.code {
                    KeyCode::Enter => {
                        let path = PathBuf::from(app.input_buf.trim());
                        if !app.input_buf.trim().is_empty() {
                            app.output_dir = Some(path.clone());
                            app.status = format!("Output: {}", path.display());
                        }
                        app.input_mode = false;
                        app.input_buf.clear();
                    }
                    KeyCode::Esc => {
                        app.input_mode = false;
                        app.input_buf.clear();
                    }
                    KeyCode::Backspace => {
                        app.input_buf.pop();
                    }
                    KeyCode::Char(c) => {
                        app.input_buf.push(c);
                    }
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Tab => {
                    app.focus = app.focus.next();
                }
                KeyCode::Char('t') => {
                    app.cumulative = !app.cumulative;
                    app.selected_files.clear();
                    app.rebuild_file_rows();
                    app.status = if app.cumulative {
                        "Cumulative view".into()
                    } else {
                        "Layer view".into()
                    };
                }
                KeyCode::Char('o') => {
                    app.input_mode = true;
                    app.input_buf.clear();
                    app.status = "Enter output directory:".into();
                }
                KeyCode::Char('j') | KeyCode::Down => match app.focus {
                    Pane::Layers => {
                        if app.layer_index < app.image.layers.len().saturating_sub(1) {
                            app.layer_index += 1;
                            app.selected_files.clear();
                            app.rebuild_file_rows();
                        }
                    }
                    Pane::Files => {
                        if app.file_index < app.file_rows.len().saturating_sub(1) {
                            app.file_index += 1;
                        }
                    }
                    Pane::Details => {}
                },
                KeyCode::Char('k') | KeyCode::Up => match app.focus {
                    Pane::Layers => {
                        if app.layer_index > 0 {
                            app.layer_index -= 1;
                            app.selected_files.clear();
                            app.rebuild_file_rows();
                        }
                    }
                    Pane::Files => {
                        if app.file_index > 0 {
                            app.file_index -= 1;
                        }
                    }
                    Pane::Details => {}
                },
                KeyCode::Enter => {
                    if app.focus == Pane::Files {
                        if let Some(row) = app.file_rows.get(app.file_index) {
                            if row.is_dir {
                                let path = row.path.clone();
                                if app.expanded_dirs.contains(&path) {
                                    app.expanded_dirs.remove(&path);
                                } else {
                                    app.expanded_dirs.insert(path);
                                }
                                app.rebuild_file_rows();
                            }
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if app.focus == Pane::Files {
                        if let Some(row) = app.file_rows.get(app.file_index) {
                            let path = row.path.clone();
                            if row.is_dir {
                                // Select/deselect all files under this dir
                                let tree = app.current_tree();
                                if let Some(node) = find_node(&tree.root, &path) {
                                    let paths = tree::collect_paths(node);
                                    let all_selected = paths.iter().all(|p| app.selected_files.contains(p));
                                    if all_selected {
                                        for p in paths {
                                            app.selected_files.remove(&p);
                                        }
                                    } else {
                                        for p in paths {
                                            app.selected_files.insert(p);
                                        }
                                    }
                                }
                            } else if app.selected_files.contains(&path) {
                                app.selected_files.remove(&path);
                            } else {
                                app.selected_files.insert(path);
                            }
                        }
                    }
                }
                KeyCode::Char('e') => {
                    match app.focus {
                        Pane::Layers => {
                            // Export all layers as tar.gz
                            let dir = app.ensure_output_dir();
                            match extract::export_all_layers(&app.image.layers, &dir) {
                                Ok(paths) => {
                                    app.status = format!(
                                        "Exported {} layers to {}",
                                        paths.len(),
                                        dir.display()
                                    );
                                }
                                Err(e) => {
                                    app.status = format!("Export error: {}", e);
                                }
                            }
                        }
                        Pane::Files => {
                            if app.selected_files.is_empty() {
                                app.status = "No files selected (use space to select)".into();
                            } else {
                                let dir = app.ensure_output_dir();
                                let paths: Vec<String> =
                                    app.selected_files.iter().cloned().collect();
                                let layer = &app.image.layers[app.layer_index];
                                match extract::extract_files(layer, &paths, &dir) {
                                    Ok(count) => {
                                        app.status = format!(
                                            "Extracted {} files to {}",
                                            count,
                                            dir.display()
                                        );
                                        app.selected_files.clear();
                                    }
                                    Err(e) => {
                                        app.status = format!("Extract error: {}", e);
                                    }
                                }
                            }
                        }
                        Pane::Details => {}
                    }
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn find_node<'a>(node: &'a FileNode, path: &str) -> Option<&'a FileNode> {
    if node.path == path {
        return Some(node);
    }
    for child in node.children.values() {
        if let Some(found) = find_node(child, path) {
            return Some(found);
        }
    }
    None
}

// ── Drawing ─────────────────────────────────────────────────────────────────

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    let main_area = chunks[0];
    let status_area = chunks[1];

    // Main: left (layers) | right (files + details)
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(main_area);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(h_chunks[1]);

    draw_layers(f, app, h_chunks[0]);
    draw_files(f, app, right_chunks[0]);
    draw_details(f, app, right_chunks[1]);
    draw_status(f, app, status_area);
}

fn draw_layers(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .image
        .layers
        .iter()
        .map(|l| {
            let cmd = truncate_command(&l.command, area.width.saturating_sub(20) as usize);
            let line = format!(
                " {} │ {:>8} │ {}",
                l.index,
                tree::human_size(l.size),
                cmd
            );
            ListItem::new(line)
        })
        .collect();

    let title = format!(" Layers ({}) ", app.image.layers.len());
    let border_style = if app.focus == Pane::Layers {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.layer_index));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_files(f: &mut Frame, app: &App, area: Rect) {
    let view_label = if app.cumulative { "cumulative" } else { "layer" };
    let title = format!(
        " Files ({}, {}) ",
        app.file_rows.len(),
        view_label
    );

    let border_style = if app.focus == Pane::Files {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app
        .file_rows
        .iter()
        .map(|row| {
            let indent = "  ".repeat(row.depth);
            let icon = if row.is_dir {
                if row.expanded { "▾ " } else { "▸ " }
            } else {
                "  "
            };

            let selected = if app.selected_files.contains(&row.path) {
                "✓ "
            } else {
                "  "
            };

            let suffix = if let Some(ref target) = row.link_target {
                format!(" -> {}", target)
            } else if row.is_dir {
                String::new()
            } else {
                format!("  {}", tree::human_size(row.size))
            };

            let line = format!("{}{}{}{}{}", selected, indent, icon, row.name, suffix);

            let style = if row.is_dir {
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
            } else if row.link_target.is_some() {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.file_index));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Pane::Details {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let layer = &app.image.layers[app.layer_index];
    let file_count = layer.file_tree.file_count;

    let text = vec![
        Line::from(vec![
            Span::styled("Command:  ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&layer.command),
        ]),
        Line::from(vec![
            Span::styled("Digest:   ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&layer.digest),
        ]),
        Line::from(vec![
            Span::styled("DiffID:   ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&layer.diff_id),
        ]),
        Line::from(vec![
            Span::styled("Size:     ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(tree::human_size(layer.size)),
        ]),
        Line::from(vec![
            Span::styled("Created:  ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&layer.created),
        ]),
        Line::from(vec![
            Span::styled("Files:    ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(file_count.to_string()),
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(" Details "),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let content = if app.input_mode {
        format!("Output dir: {}█", app.input_buf)
    } else if app.status.is_empty() {
        format!(
            " {} │ {} │ {} │ q:quit tab:focus j/k:nav enter:expand space:select e:extract t:toggle o:output",
            app.image.source,
            app.image.architecture,
            tree::human_size(app.image.total_size),
        )
    } else {
        format!(" {}", app.status)
    };

    let bar = Paragraph::new(content)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(bar, area);
}

fn truncate_command(cmd: &str, max_len: usize) -> String {
    // Strip common prefixes for readability
    let cleaned = cmd
        .replace("/bin/sh -c ", "")
        .replace("#(nop) ", "");
    if cleaned.len() > max_len {
        format!("{}…", &cleaned[..max_len.saturating_sub(1)])
    } else {
        cleaned
    }
}
