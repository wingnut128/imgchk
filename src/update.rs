use std::ops::ControlFlow;
use std::path::PathBuf;

use crate::action::Action;
use crate::extract::{self, OutputFormat};
use crate::ui::App;

/// Apply an action to the app. Returns `Break` only on `Action::Quit`.
///
/// Extraction actions perform I/O inline. Pure-state actions (navigation,
/// toggles, modal-input lifecycle) are unit-testable without fixtures.
pub fn update(app: &mut App, action: Action) -> ControlFlow<()> {
    match action {
        Action::Quit => return ControlFlow::Break(()),

        Action::CyclePaneFocus => {
            app.focus = app.focus.next();
            app.status.clear();
        }

        Action::ToggleCumulative => {
            app.cumulative = !app.cumulative;
            app.selection.clear();
            app.rebuild_file_rows();
            app.status = if app.cumulative {
                "Cumulative view".into()
            } else {
                "Layer view".into()
            };
        }

        Action::CycleOutputFormat => {
            app.output_format = app.output_format.next();
            app.status = format!("Export format: {}", app.output_format.label());
        }

        Action::EnterOutputDirInput => {
            app.input_mode = true;
            app.input_buf.clear();
            app.status = "Enter output directory:".into();
        }

        Action::SubmitInput => {
            let trimmed = app.input_buf.trim();
            if !trimmed.is_empty() {
                let path = PathBuf::from(trimmed);
                app.status = format!("Output: {}", path.display());
                app.output_dir = Some(path);
            }
            app.input_mode = false;
            app.input_buf.clear();
        }

        Action::CancelInput => {
            app.input_mode = false;
            app.input_buf.clear();
        }

        Action::InputBackspace => {
            app.input_buf.pop();
        }

        Action::InputChar(c) => {
            app.input_buf.push(c);
        }

        Action::NavigateLayer { delta } => {
            let layer_count = app.image.layers.len();
            if layer_count == 0 {
                return ControlFlow::Continue(());
            }
            let new_index =
                (app.layer_index as i64 + delta as i64).clamp(0, layer_count as i64 - 1) as usize;
            if new_index != app.layer_index {
                app.layer_index = new_index;
                app.selection.clear();
                app.detail_scroll = 0;
                app.rebuild_file_rows();
            }
            app.status.clear();
        }

        Action::NavigateFile { delta } => {
            let row_count = app.file_rows.len();
            if row_count == 0 {
                app.status.clear();
                return ControlFlow::Continue(());
            }
            let new_index =
                (app.file_index as i64 + delta as i64).clamp(0, row_count as i64 - 1) as usize;
            app.file_index = new_index;
            app.status.clear();
        }

        Action::ScrollDetails { delta } => {
            if delta >= 0 {
                app.detail_scroll = app.detail_scroll.saturating_add(delta as u16);
            } else {
                app.detail_scroll = app.detail_scroll.saturating_sub((-delta) as u16);
            }
            app.status.clear();
        }

        Action::ToggleExpand => {
            if let Some(row) = app.file_rows.get(app.file_index)
                && row.is_dir
            {
                let path = row.path.clone();
                if app.expanded_dirs.contains(&path) {
                    app.expanded_dirs.remove(&path);
                } else {
                    app.expanded_dirs.insert(path);
                }
                app.rebuild_file_rows();
            }
            app.status.clear();
        }

        Action::ToggleSelection => {
            if let Some(row) = app.file_rows.get(app.file_index) {
                let path = row.path.clone();
                if row.is_dir {
                    app.selection.toggle_under(&path, &app.cached_tree);
                } else {
                    app.selection.toggle_file(&path, &app.cached_tree);
                }
            }
        }

        Action::ExtractCurrentLayer => {
            let dir = app.ensure_output_dir();
            let fmt = app.output_format;
            let layer = &app.image.layers[app.layer_index];
            let result = match fmt {
                OutputFormat::TarGz => extract::export_layer(layer, &dir)
                    .map(|path| format!("Exported layer {} to {}", layer.index, path.display())),
                _ => {
                    let name = format!("layer-{}", layer.index);
                    let spec = extract::make_image_spec(fmt, &dir, &name);
                    extract::export_ocirender_single(layer, spec).map(|path| {
                        format!(
                            "Exported layer {} as {} to {}",
                            layer.index,
                            fmt.label(),
                            path.display()
                        )
                    })
                }
            };
            app.status = match result {
                Ok(msg) => msg,
                Err(e) => format!("Export error: {}", e),
            };
        }

        Action::ExtractAllLayers => {
            let dir = app.ensure_output_dir();
            let fmt = app.output_format;
            let result = match fmt {
                OutputFormat::TarGz => extract::export_all_layers(&app.image.layers, &dir)
                    .map(|paths| format!("Exported {} layers to {}", paths.len(), dir.display())),
                _ => {
                    let spec = extract::make_image_spec(fmt, &dir, "image");
                    extract::export_ocirender(&app.image.layers, spec).map(|path| {
                        format!(
                            "Exported all layers as {} to {}",
                            fmt.label(),
                            path.display()
                        )
                    })
                }
            };
            app.status = match result {
                Ok(msg) => msg,
                Err(e) => format!("Export error: {}", e),
            };
        }

        Action::ExtractFiles => {
            let dir = app.ensure_output_dir();
            let paths: Vec<String> = if app.selection.is_empty() {
                app.cached_tree.all_paths()
            } else {
                app.selection.paths().iter().cloned().collect()
            };
            let label = if app.selection.is_empty() {
                "all"
            } else {
                "selected"
            };
            let layer = &app.image.layers[app.layer_index];
            match extract::extract_files(layer, &paths, &dir) {
                Ok(count) => {
                    app.status =
                        format!("Extracted {} {} files to {}", count, label, dir.display());
                    // TODO: revisit whether selection should survive extraction
                    // so users can re-extract with a different format. Today's
                    // behavior matches the pre-refactor code.
                    app.selection.clear();
                }
                Err(e) => {
                    app.status = format!("Extract error: {}", e);
                }
            }
        }
    }

    ControlFlow::Continue(())
}

#[cfg(test)]
#[allow(unused_must_use)]
mod tests {
    use super::*;
    use crate::image::{ImageInfo, LayerInfo};
    use crate::tree::{FileNode, FileTree};
    use crate::ui::Pane;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn empty_tree() -> FileTree {
        FileTree::new()
    }

    fn tree_with(paths: &[&str]) -> FileTree {
        let mut t = FileTree::new();
        for p in paths {
            let name = p.rsplit('/').next().unwrap_or(p).to_string();
            t.insert_node(
                p,
                FileNode {
                    name,
                    path: (*p).into(),
                    size: 1,
                    mode: 0o644,
                    is_dir: false,
                    is_whiteout: false,
                    is_opaque: false,
                    link_target: None,
                    children: BTreeMap::new(),
                },
            );
            t.file_count += 1;
            t.total_size += 1;
        }
        t
    }

    fn layer(index: usize, tree: FileTree) -> LayerInfo {
        LayerInfo {
            index,
            digest: format!("sha256:{}", "0".repeat(64)),
            diff_id: format!("sha256:{}", "0".repeat(64)),
            size: tree.total_size,
            command: String::new(),
            created: String::new(),
            media_type: "application/vnd.oci.image.layer.v1.tar".into(),
            blob_path: PathBuf::from("/tmp/none"),
            file_tree: tree,
        }
    }

    fn app_with_layers(layers: Vec<LayerInfo>) -> App {
        let image = ImageInfo {
            source: "test".into(),
            os: "linux".into(),
            architecture: "amd64".into(),
            total_size: layers.iter().map(|l| l.size).sum(),
            layers,
        };
        App::new(image, None)
    }

    fn single_empty_layer_app() -> App {
        app_with_layers(vec![layer(0, empty_tree())])
    }

    // ── Quit ──────────────────────────────────────────────────────────────

    #[test]
    fn quit_breaks() {
        let mut app = single_empty_layer_app();
        assert!(matches!(
            update(&mut app, Action::Quit),
            ControlFlow::Break(())
        ));
    }

    // ── Focus ─────────────────────────────────────────────────────────────

    #[test]
    fn cycle_pane_focus_advances_and_clears_status() {
        let mut app = single_empty_layer_app();
        app.status = "lingering".into();
        update(&mut app, Action::CyclePaneFocus);
        assert_eq!(app.focus, Pane::Files);
        assert!(app.status.is_empty());
        update(&mut app, Action::CyclePaneFocus);
        assert_eq!(app.focus, Pane::Details);
        update(&mut app, Action::CyclePaneFocus);
        assert_eq!(app.focus, Pane::Layers);
    }

    // ── Cumulative toggle ─────────────────────────────────────────────────

    #[test]
    fn toggle_cumulative_flips_and_sets_status() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"]))]);
        assert!(!app.cumulative);
        update(&mut app, Action::ToggleCumulative);
        assert!(app.cumulative);
        assert_eq!(app.status, "Cumulative view");
        update(&mut app, Action::ToggleCumulative);
        assert!(!app.cumulative);
        assert_eq!(app.status, "Layer view");
    }

    #[test]
    fn toggle_cumulative_clears_selection() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"]))]);
        app.selection.toggle_file("/a", &app.cached_tree.clone());
        assert!(!app.selection.is_empty());
        update(&mut app, Action::ToggleCumulative);
        assert!(app.selection.is_empty());
    }

    // ── Format cycle ──────────────────────────────────────────────────────

    #[test]
    fn cycle_format_advances() {
        let mut app = single_empty_layer_app();
        let initial = app.output_format;
        update(&mut app, Action::CycleOutputFormat);
        assert_ne!(app.output_format, initial);
        assert!(app.status.starts_with("Export format:"));
    }

    // ── Modal input ───────────────────────────────────────────────────────

    #[test]
    fn enter_output_dir_input_opens_modal() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        assert!(app.input_mode);
        assert!(app.input_buf.is_empty());
        assert_eq!(app.status, "Enter output directory:");
    }

    #[test]
    fn input_chars_append_to_buffer() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        update(&mut app, Action::InputChar('/'));
        update(&mut app, Action::InputChar('t'));
        update(&mut app, Action::InputChar('m'));
        update(&mut app, Action::InputChar('p'));
        assert_eq!(app.input_buf, "/tmp");
    }

    #[test]
    fn input_backspace_pops_last_char() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        update(&mut app, Action::InputChar('a'));
        update(&mut app, Action::InputChar('b'));
        update(&mut app, Action::InputBackspace);
        assert_eq!(app.input_buf, "a");
    }

    #[test]
    fn submit_input_sets_output_dir() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        for c in "/tmp/out".chars() {
            update(&mut app, Action::InputChar(c));
        }
        update(&mut app, Action::SubmitInput);
        assert!(!app.input_mode);
        assert!(app.input_buf.is_empty());
        assert_eq!(app.output_dir, Some(PathBuf::from("/tmp/out")));
    }

    #[test]
    fn submit_empty_input_keeps_existing_output_dir() {
        let mut app = single_empty_layer_app();
        app.output_dir = Some(PathBuf::from("/existing"));
        update(&mut app, Action::EnterOutputDirInput);
        update(&mut app, Action::SubmitInput);
        assert_eq!(app.output_dir, Some(PathBuf::from("/existing")));
    }

    #[test]
    fn cancel_input_exits_without_changing_output_dir() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        update(&mut app, Action::InputChar('/'));
        update(&mut app, Action::CancelInput);
        assert!(!app.input_mode);
        assert!(app.input_buf.is_empty());
        assert_eq!(app.output_dir, None);
    }

    // ── Layer navigation ──────────────────────────────────────────────────

    #[test]
    fn navigate_layer_clamps_to_bounds() {
        let mut app = app_with_layers(vec![
            layer(0, empty_tree()),
            layer(1, empty_tree()),
            layer(2, empty_tree()),
        ]);
        update(&mut app, Action::NavigateLayer { delta: 1 });
        assert_eq!(app.layer_index, 1);
        update(&mut app, Action::NavigateLayer { delta: 10 });
        assert_eq!(app.layer_index, 2);
        update(&mut app, Action::NavigateLayer { delta: -100 });
        assert_eq!(app.layer_index, 0);
    }

    #[test]
    fn navigate_layer_clears_selection_and_resets_detail_scroll() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"])), layer(1, empty_tree())]);
        app.selection.toggle_file("/a", &app.cached_tree.clone());
        app.detail_scroll = 5;
        update(&mut app, Action::NavigateLayer { delta: 1 });
        assert_eq!(app.layer_index, 1);
        assert!(app.selection.is_empty());
        assert_eq!(app.detail_scroll, 0);
    }

    // ── File navigation ──────────────────────────────────────────────────

    #[test]
    fn navigate_file_clamps_to_row_count() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a", "/b", "/c"]))]);
        // file_rows has 3 entries
        update(&mut app, Action::NavigateFile { delta: 1 });
        assert_eq!(app.file_index, 1);
        update(&mut app, Action::NavigateFile { delta: 100 });
        assert_eq!(app.file_index, app.file_rows.len() - 1);
        update(&mut app, Action::NavigateFile { delta: -100 });
        assert_eq!(app.file_index, 0);
    }

    #[test]
    fn navigate_file_with_no_rows_is_safe() {
        let mut app = single_empty_layer_app();
        assert!(app.file_rows.is_empty());
        update(&mut app, Action::NavigateFile { delta: 1 });
        assert_eq!(app.file_index, 0);
    }

    // ── Details scroll ────────────────────────────────────────────────────

    #[test]
    fn scroll_details_saturates() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::ScrollDetails { delta: 5 });
        assert_eq!(app.detail_scroll, 5);
        update(&mut app, Action::ScrollDetails { delta: -2 });
        assert_eq!(app.detail_scroll, 3);
        update(&mut app, Action::ScrollDetails { delta: -100 });
        assert_eq!(app.detail_scroll, 0);
    }

    // ── Toggle expand ────────────────────────────────────────────────────

    #[test]
    fn toggle_expand_on_dir_toggles_membership() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/d/x"]))]);
        // file_rows[0] is /d (a dir, the only top-level row when /d is collapsed).
        let dir_path = app.file_rows[0].path.clone();
        assert!(app.file_rows[0].is_dir);
        update(&mut app, Action::ToggleExpand);
        assert!(app.expanded_dirs.contains(&dir_path));
        update(&mut app, Action::ToggleExpand);
        assert!(!app.expanded_dirs.contains(&dir_path));
    }

    #[test]
    fn toggle_expand_on_file_is_noop() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"]))]);
        // /a is a file, not a dir
        assert!(!app.file_rows[0].is_dir);
        update(&mut app, Action::ToggleExpand);
        assert!(app.expanded_dirs.is_empty());
    }

    // ── Toggle selection ──────────────────────────────────────────────────

    #[test]
    fn toggle_selection_on_file_toggles_membership() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"]))]);
        update(&mut app, Action::ToggleSelection);
        assert!(app.selection.contains("/a"));
        update(&mut app, Action::ToggleSelection);
        assert!(!app.selection.contains("/a"));
    }
}
