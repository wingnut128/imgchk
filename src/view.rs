use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::command_format::{clean_command, format_command, truncate_command};
use crate::selection::DirStatus;
use crate::tree;
use crate::ui::{App, Pane};

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

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    let main_area = chunks[0];
    let status_area = chunks[1];

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

    let mut scrollbar_state = ScrollbarState::new(app.image.layers.len()).position(app.layer_index);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(Color::DarkGray)),
        area.inner(Margin::new(0, 1)),
        &mut scrollbar_state,
    );
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
                if row.expanded { "▾ " } else { "▸ " }
            } else {
                "  "
            };

            let (is_selected, is_partial) = if row.is_dir {
                match app.selection.dir_status(&row.path) {
                    DirStatus::All => (true, false),
                    DirStatus::Partial => (false, true),
                    DirStatus::None => (false, false),
                }
            } else {
                (app.selection.contains(&row.path), false)
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

    let mut scrollbar_state = ScrollbarState::new(app.file_rows.len()).position(app.file_index);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(Color::DarkGray)),
        area.inner(Margin::new(0, 1)),
        &mut scrollbar_state,
    );
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

    lines.push(Line::from(""));

    let command = clean_command(&layer.command);
    let cmd_lines = format_command(&command);
    let keyword_style = Style::default()
        .fg(Color::LightCyan)
        .add_modifier(Modifier::BOLD);
    let op_style = Style::default().fg(Color::Yellow);

    for (i, cmd_line) in cmd_lines.iter().enumerate() {
        let trimmed = cmd_line.trim();
        let spans = highlight_shell_line(trimmed, keyword_style, op_style, value_style);
        let mut line_spans = if i == 0 {
            vec![Span::styled("$ ", label_style)]
        } else {
            vec![Span::raw("  ")]
        };
        line_spans.extend(spans);
        lines.push(Line::from(line_spans));
    }

    let total_lines = lines.len();

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title_style(title_style)
                .title(" Details "),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));

    f.render_widget(paragraph, area);

    let mut scrollbar_state = ScrollbarState::new(total_lines).position(app.detail_scroll as usize);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(Color::DarkGray)),
        area.inner(Margin::new(0, 1)),
        &mut scrollbar_state,
    );
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let content = if app.input_mode {
        format!("Output dir: {}█", app.input_buf)
    } else if app.status.is_empty() {
        format!(
            " {} │ {}/{} │ {} │ fmt:{} │ q:quit tab j/k e:extract a:all f:format t:toggle o:output",
            app.image.source,
            app.image.os,
            app.image.architecture,
            tree::human_size(app.image.total_size),
            app.output_format.label(),
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

fn highlight_shell_line<'a>(
    line: &'a str,
    keyword_style: Style,
    op_style: Style,
    default_style: Style,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

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
