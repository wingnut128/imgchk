use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
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
    cached_tree: FileTree,
    file_rows: Vec<TreeRow>,
    file_index: usize,
    selected_files: HashSet<String>,
    /// Pre-computed per-directory selection state: (all_selected, some_selected)
    dir_selection: std::collections::HashMap<String, (bool, bool)>,

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
            cached_tree: FileTree::new(),
            file_rows: Vec::new(),
            file_index: 0,
            selected_files: HashSet::new(),
            dir_selection: std::collections::HashMap::new(),
            output_dir,
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
        } else {
            self.image.layers[self.layer_index].file_tree.clone()
        };
    }

    fn rebuild_file_rows(&mut self) {
        self.rebuild_tree();
        let mut rows = Vec::new();
        flatten_node(&self.cached_tree.root, 0, &self.expanded_dirs, &mut rows);
        self.file_rows = rows;
        if self.file_index >= self.file_rows.len() {
            self.file_index = self.file_rows.len().saturating_sub(1);
        }
        self.rebuild_dir_selection();
    }

    fn rebuild_dir_selection(&mut self) {
        self.dir_selection.clear();
        self.compute_dir_selection(&self.cached_tree.root.clone());
    }

    fn compute_dir_selection(&mut self, node: &FileNode) {
        for child in node.children.values() {
            if child.is_dir {
                let paths = tree::collect_paths(child);
                if !paths.is_empty() {
                    let sel = paths
                        .iter()
                        .filter(|p| self.selected_files.contains(*p))
                        .count();
                    let all = sel == paths.len();
                    let some = sel > 0 && !all;
                    self.dir_selection.insert(child.path.clone(), (all, some));
                }
                self.compute_dir_selection(child);
            }
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

fn flatten_node(
    node: &FileNode,
    depth: usize,
    expanded: &HashSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    // Sort: dirs first, then by name
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

            // Clear transient status on any navigation
            match key.code {
                KeyCode::Char('j' | 'k')
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Tab
                | KeyCode::Enter => {
                    app.status.clear();
                }
                _ => {}
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
                                if let Some(node) = find_node(&app.cached_tree.root, &path) {
                                    let paths = tree::collect_paths(node);
                                    let all_selected =
                                        paths.iter().all(|p| app.selected_files.contains(p));
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
                            app.rebuild_dir_selection();
                        }
                    }
                }
                KeyCode::Char('a') => {
                    if app.focus == Pane::Layers {
                        let dir = app.ensure_output_dir();
                        match extract::export_all_layers(&app.image.layers, &dir) {
                            Ok(paths) => {
                                app.status =
                                    format!("Exported {} layers to {}", paths.len(), dir.display());
                            }
                            Err(e) => {
                                app.status = format!("Export error: {}", e);
                            }
                        }
                    }
                }
                KeyCode::Char('e') => match app.focus {
                    Pane::Layers => {
                        let dir = app.ensure_output_dir();
                        let layer = &app.image.layers[app.layer_index];
                        match extract::export_layer(layer, &dir) {
                            Ok(path) => {
                                app.status =
                                    format!("Exported layer {} to {}", layer.index, path.display());
                            }
                            Err(e) => {
                                app.status = format!("Export error: {}", e);
                            }
                        }
                    }
                    Pane::Files => {
                        let dir = app.ensure_output_dir();
                        let paths: Vec<String> = if app.selected_files.is_empty() {
                            tree::collect_paths(&app.cached_tree.root)
                        } else {
                            app.selected_files.iter().cloned().collect()
                        };
                        let label = if app.selected_files.is_empty() {
                            "all"
                        } else {
                            "selected"
                        };
                        let layer = &app.image.layers[app.layer_index];
                        match extract::extract_files(layer, &paths, &dir) {
                            Ok(count) => {
                                app.status = format!(
                                    "Extracted {} {} files to {}",
                                    count,
                                    label,
                                    dir.display()
                                );
                                app.selected_files.clear();
                            }
                            Err(e) => {
                                app.status = format!("Extract error: {}", e);
                            }
                        }
                    }
                    Pane::Details => {}
                },
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

const HIGHLIGHT_STYLE: Style = Style::new()
    .bg(Color::Rgb(50, 50, 80))
    .fg(Color::White)
    .add_modifier(Modifier::BOLD);

fn focused_styles(is_focused: bool) -> (Style, Style) {
    if is_focused {
        (
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
            // Inverted title for clear focus indicator
            Style::default()
                .bg(Color::LightYellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(Color::Gray),
            Style::default().fg(Color::White),
        )
    }
}

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
            let line = format!(" {} │ {:>8} │ {}", l.index, tree::human_size(l.size), cmd);
            ListItem::new(line)
        })
        .collect();

    let title = format!(" Layers ({}) ", app.image.layers.len());
    let (border_style, title_style) = focused_styles(app.focus == Pane::Layers);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title_style(title_style)
                .title(title),
        )
        .highlight_style(HIGHLIGHT_STYLE)
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.layer_index));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_files(f: &mut Frame, app: &App, area: Rect) {
    let view_label = if app.cumulative {
        "cumulative"
    } else {
        "layer"
    };
    let title = format!(" Files ({}, {}) ", app.file_rows.len(), view_label);

    let (border_style, title_style) = focused_styles(app.focus == Pane::Files);

    let items: Vec<ListItem> = app
        .file_rows
        .iter()
        .map(|row| {
            let indent = "  ".repeat(row.depth);
            let icon = if row.is_dir {
                if row.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };

            let (is_selected, is_partial) = if row.is_dir {
                app.dir_selection
                    .get(&row.path)
                    .copied()
                    .unwrap_or((false, false))
            } else {
                (app.selected_files.contains(&row.path), false)
            };

            let suffix = if let Some(ref target) = row.link_target {
                format!(" -> {}", target)
            } else if row.is_dir {
                String::new()
            } else {
                format!("  {}", tree::human_size(row.size))
            };

            let checkbox = if is_selected {
                Span::styled(
                    "[✓] ",
                    Style::default()
                        .fg(Color::LightGreen)
                        .add_modifier(Modifier::BOLD),
                )
            } else if is_partial {
                Span::styled(
                    "[~] ",
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("[ ] ", Style::default().fg(Color::Gray))
            };

            let name_style = if is_selected {
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD)
            } else if is_partial {
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD)
            } else if row.is_dir {
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD)
            } else if row.link_target.is_some() {
                Style::default().fg(Color::LightCyan)
            } else {
                Style::default().fg(Color::White)
            };

            let line = Line::from(vec![
                checkbox,
                Span::raw(format!("{}{}", indent, icon)),
                Span::styled(&row.name, name_style),
                Span::styled(suffix, Style::default().fg(Color::Gray)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title_style(title_style)
                .title(title),
        )
        .highlight_style(HIGHLIGHT_STYLE)
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.file_index));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let (border_style, title_style) = focused_styles(app.focus == Pane::Details);

    let layer = &app.image.layers[app.layer_index];
    let file_count = layer.file_tree.file_count;

    let label_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(Color::White);
    let dim_style = Style::default().fg(Color::Gray);

    // Compact metadata header
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("Size: ", label_style),
            Span::styled(tree::human_size(layer.size), value_style),
            Span::styled("  Files: ", label_style),
            Span::styled(file_count.to_string(), value_style),
            Span::styled("  Created: ", label_style),
            Span::styled(&layer.created, dim_style),
        ]),
        Line::from(vec![
            Span::styled("Digest: ", label_style),
            Span::styled(&layer.digest, dim_style),
        ]),
    ];

    if !layer.diff_id.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("DiffID: ", label_style),
            Span::styled(&layer.diff_id, dim_style),
        ]));
    }

    // Separator
    lines.push(Line::from(""));

    // Pretty-print the command
    let command = clean_command(&layer.command);
    let cmd_lines = format_command(&command);
    let keyword_style = Style::default()
        .fg(Color::LightCyan)
        .add_modifier(Modifier::BOLD);
    let op_style = Style::default().fg(Color::Yellow);

    for (i, cmd_line) in cmd_lines.iter().enumerate() {
        let trimmed = cmd_line.trim();
        if i == 0 {
            // First line gets the label
            let spans = highlight_shell_line(trimmed, keyword_style, op_style, value_style);
            let mut line_spans = vec![Span::styled("$ ", label_style)];
            line_spans.extend(spans);
            lines.push(Line::from(line_spans));
        } else {
            let spans = highlight_shell_line(trimmed, keyword_style, op_style, value_style);
            let mut line_spans = vec![Span::raw("  ")];
            line_spans.extend(spans);
            lines.push(Line::from(line_spans));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title_style(title_style)
                .title(" Details "),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn format_command(cmd: &str) -> Vec<String> {
    // Split at shell separators, keeping the separator at the start of the next line
    let mut lines = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = cmd.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        if c == ';' {
            current.push(';');
            lines.push(current.trim().to_string());
            current = String::new();
            i += 1;
        } else if c == '&' && i + 1 < len && chars[i + 1] == '&' {
            current.push_str("&&");
            lines.push(current.trim().to_string());
            current = String::new();
            i += 2;
        } else if c == '|' && i + 1 < len && chars[i + 1] == '|' {
            current.push_str("||");
            lines.push(current.trim().to_string());
            current = String::new();
            i += 2;
        } else if c == '|' && (i + 1 >= len || chars[i + 1] != '|') {
            // Pipe — keep on same line but break after
            current.push('|');
            lines.push(current.trim().to_string());
            current = String::new();
            i += 1;
        } else {
            current.push(c);
            i += 1;
        }
    }

    let remaining = current.trim().to_string();
    if !remaining.is_empty() {
        lines.push(remaining);
    }

    if lines.is_empty() {
        lines.push(cmd.to_string());
    }

    lines
}

fn highlight_shell_line<'a>(
    line: &'a str,
    keyword_style: Style,
    op_style: Style,
    default_style: Style,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

    // Highlight shell keywords and operators
    let first_word = line.split_whitespace().next().unwrap_or("");
    let is_keyword = matches!(
        first_word,
        "RUN"
            | "CMD"
            | "COPY"
            | "ADD"
            | "ENV"
            | "WORKDIR"
            | "EXPOSE"
            | "FROM"
            | "ARG"
            | "LABEL"
            | "ENTRYPOINT"
            | "VOLUME"
            | "USER"
            | "SHELL"
            | "STOPSIGNAL"
            | "HEALTHCHECK"
            | "ONBUILD"
    );

    if is_keyword {
        spans.push(Span::styled(&line[..first_word.len()], keyword_style));
        let rest = &line[first_word.len()..];
        spans.extend(highlight_operators(rest, op_style, default_style));
    } else {
        spans.extend(highlight_operators(line, op_style, default_style));
    }

    spans
}

fn highlight_operators<'a>(text: &'a str, op_style: Style, default_style: Style) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let mut last = 0;

    let ops = ["&&", "||", "|", ">>", ">&", ">", "<"];

    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let mut matched = false;
        for op in &ops {
            let op_bytes = op.as_bytes();
            if i + op_bytes.len() <= len && &bytes[i..i + op_bytes.len()] == op_bytes {
                if last < i {
                    spans.push(Span::styled(&text[last..i], default_style));
                }
                spans.push(Span::styled(&text[i..i + op_bytes.len()], op_style));
                i += op_bytes.len();
                last = i;
                matched = true;
                break;
            }
        }
        if !matched {
            i += 1;
        }
    }

    if last < len {
        spans.push(Span::styled(&text[last..], default_style));
    }

    spans
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let content = if app.input_mode {
        format!("Output dir: {}█", app.input_buf)
    } else if app.status.is_empty() {
        format!(
            " {} │ {}/{} │ {} │ q:quit tab:focus j/k:nav enter:expand space:select e:extract a:all t:toggle o:output",
            app.image.source,
            app.image.os,
            app.image.architecture,
            tree::human_size(app.image.total_size),
        )
    } else {
        format!(" {}", app.status)
    };

    let bar = Paragraph::new(content).style(
        Style::default()
            .bg(Color::Rgb(40, 40, 60))
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(bar, area);
}

fn clean_command(cmd: &str) -> String {
    let stripped = cmd.replace("/bin/sh -c ", "").replace("#(nop) ", "");
    // Collapse internal whitespace (newlines, tabs, multiple spaces)
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_command(cmd: &str, max_len: usize) -> String {
    let cleaned = clean_command(cmd);
    if cleaned.len() > max_len {
        format!("{}…", &cleaned[..max_len.saturating_sub(1)])
    } else {
        cleaned
    }
}
