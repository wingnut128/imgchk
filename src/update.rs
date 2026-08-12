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
            app.output.status.clear();
        }

        Action::ToggleCumulative => {
            app.nav.cumulative = !app.nav.cumulative;
            app.selection.clear();
            app.rebuild_file_rows();
            app.output.status = if app.nav.cumulative {
                "Cumulative view".into()
            } else {
                "Layer view".into()
            };
        }

        Action::CycleOutputFormat => {
            app.output.format = app.output.format.next();
            app.output.status = format!("Export format: {}", app.output.format.label());
        }

        Action::EnterOutputDirInput => {
            app.modal.active = true;
            app.modal.buffer.clear();
            app.output.status = "Enter output directory:".into();
        }

        Action::SubmitInput => {
            let trimmed = app.modal.buffer.trim();
            if !trimmed.is_empty() {
                let path = PathBuf::from(trimmed);
                app.output.status = format!("Output: {}", path.display());
                app.output.dir = Some(path);
            }
            app.modal.active = false;
            app.modal.buffer.clear();
        }

        Action::CancelInput => {
            app.modal.active = false;
            app.modal.buffer.clear();
        }

        Action::InputBackspace => {
            app.modal.buffer.pop();
        }

        Action::InputChar(c) => {
            app.modal.buffer.push(c);
        }

        Action::NavigateLayer { delta } => {
            let layer_count = app.image.layers.len();
            if layer_count == 0 {
                return ControlFlow::Continue(());
            }
            let new_index = (app.nav.layer_index as i64 + delta as i64)
                .clamp(0, layer_count as i64 - 1) as usize;
            if new_index != app.nav.layer_index {
                app.nav.layer_index = new_index;
                app.selection.clear();
                app.nav.detail_scroll = 0;
                app.rebuild_file_rows();
            }
            app.output.status.clear();
        }

        Action::NavigateFile { delta } => {
            let row_count = app.nav.file_rows.len();
            if row_count == 0 {
                app.output.status.clear();
                return ControlFlow::Continue(());
            }
            let new_index =
                (app.nav.file_index as i64 + delta as i64).clamp(0, row_count as i64 - 1) as usize;
            app.nav.file_index = new_index;
            app.output.status.clear();
        }

        Action::ScrollDetails { delta } => {
            if delta >= 0 {
                app.nav.detail_scroll = app.nav.detail_scroll.saturating_add(delta as u16);
            } else {
                app.nav.detail_scroll = app.nav.detail_scroll.saturating_sub((-delta) as u16);
            }
            app.output.status.clear();
        }

        Action::ToggleExpand => {
            if let Some(row) = app.nav.file_rows.get(app.nav.file_index)
                && row.is_dir
            {
                let path = row.path.clone();
                if app.nav.expanded_dirs.contains(&path) {
                    app.nav.expanded_dirs.remove(&path);
                } else {
                    app.nav.expanded_dirs.insert(path);
                }
                app.rebuild_file_rows();
            }
            app.output.status.clear();
        }

        Action::ToggleSelection => {
            if let Some(row) = app.nav.file_rows.get(app.nav.file_index) {
                let path = row.path.clone();
                if row.is_dir {
                    app.selection.toggle_under(&path, &app.nav.cached_tree);
                } else {
                    app.selection.toggle_file(&path, &app.nav.cached_tree);
                }
            }
        }

        Action::ExtractCurrentLayer => match app.ensure_output_dir() {
            Ok(dir) => {
                let fmt = app.output.format;
                let layer = &app.image.layers[app.nav.layer_index];
                let result = match fmt {
                    OutputFormat::TarGz => extract::export_layer(layer, &dir).map(|path| {
                        format!("Exported layer {} to {}", layer.index, path.display())
                    }),
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
                app.output.status = match result {
                    Ok(msg) => msg,
                    Err(e) => format!("Export error: {e}"),
                };
            }
            Err(e) => {
                app.output.status = e;
            }
        },

        Action::ExtractAllLayers => match app.ensure_output_dir() {
            Ok(dir) => {
                let fmt = app.output.format;
                let result = match fmt {
                    OutputFormat::TarGz => {
                        extract::export_all_layers(&app.image.layers, &dir).map(|paths| {
                            format!("Exported {} layers to {}", paths.len(), dir.display())
                        })
                    }
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
                app.output.status = match result {
                    Ok(msg) => msg,
                    Err(e) => format!("Export error: {e}"),
                };
            }
            Err(e) => {
                app.output.status = e;
            }
        },

        Action::ExtractFiles => {
            match app.ensure_output_dir() {
                Ok(dir) => {
                    let paths: Vec<String> = if app.selection.is_empty() {
                        app.nav.cached_tree.all_paths()
                    } else {
                        app.selection.paths().iter().cloned().collect()
                    };
                    let label = if app.selection.is_empty() {
                        "all"
                    } else {
                        "selected"
                    };
                    let layer = &app.image.layers[app.nav.layer_index];
                    let base_name = format!("layer-{}-files", layer.index);
                    match extract::extract_files(layer, &paths, &dir, app.output.format, &base_name)
                    {
                        Ok((count, outputs)) => {
                            let dest = match outputs.as_slice() {
                                [single] => single.display().to_string(),
                                _ => dir.display().to_string(),
                            };
                            app.output.status =
                                format!("Extracted {count} {label} files to {dest}");
                            // TODO: revisit whether selection should survive extraction
                            // so users can re-extract with a different format. Today's
                            // behavior matches the pre-refactor code.
                            app.selection.clear();
                        }
                        Err(e) => {
                            app.output.status = format!("Extract error: {e}");
                        }
                    }
                }
                Err(e) => {
                    app.output.status = e;
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
                    is_special: false,
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
            history: Vec::new(),
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
        app.output.status = "lingering".into();
        update(&mut app, Action::CyclePaneFocus);
        assert_eq!(app.focus, Pane::Files);
        assert!(app.output.status.is_empty());
        update(&mut app, Action::CyclePaneFocus);
        assert_eq!(app.focus, Pane::Details);
        update(&mut app, Action::CyclePaneFocus);
        assert_eq!(app.focus, Pane::Layers);
    }

    // ── Cumulative toggle ─────────────────────────────────────────────────

    #[test]
    fn toggle_cumulative_flips_and_sets_status() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"]))]);
        assert!(!app.nav.cumulative);
        update(&mut app, Action::ToggleCumulative);
        assert!(app.nav.cumulative);
        assert_eq!(app.output.status, "Cumulative view");
        update(&mut app, Action::ToggleCumulative);
        assert!(!app.nav.cumulative);
        assert_eq!(app.output.status, "Layer view");
    }

    #[test]
    fn toggle_cumulative_clears_selection() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"]))]);
        app.selection
            .toggle_file("/a", &app.nav.cached_tree.clone());
        assert!(!app.selection.is_empty());
        update(&mut app, Action::ToggleCumulative);
        assert!(app.selection.is_empty());
    }

    // ── Format cycle ──────────────────────────────────────────────────────

    #[test]
    fn cycle_format_advances() {
        let mut app = single_empty_layer_app();
        let initial = app.output.format;
        update(&mut app, Action::CycleOutputFormat);
        assert_ne!(app.output.format, initial);
        assert!(app.output.status.starts_with("Export format:"));
    }

    // ── Modal input ───────────────────────────────────────────────────────

    #[test]
    fn enter_output_dir_input_opens_modal() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        assert!(app.modal.active);
        assert!(app.modal.buffer.is_empty());
        assert_eq!(app.output.status, "Enter output directory:");
    }

    #[test]
    fn input_chars_append_to_buffer() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        update(&mut app, Action::InputChar('/'));
        update(&mut app, Action::InputChar('t'));
        update(&mut app, Action::InputChar('m'));
        update(&mut app, Action::InputChar('p'));
        assert_eq!(app.modal.buffer, "/tmp");
    }

    #[test]
    fn input_backspace_pops_last_char() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        update(&mut app, Action::InputChar('a'));
        update(&mut app, Action::InputChar('b'));
        update(&mut app, Action::InputBackspace);
        assert_eq!(app.modal.buffer, "a");
    }

    #[test]
    fn submit_input_sets_output_dir() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        for c in "/tmp/out".chars() {
            update(&mut app, Action::InputChar(c));
        }
        update(&mut app, Action::SubmitInput);
        assert!(!app.modal.active);
        assert!(app.modal.buffer.is_empty());
        assert_eq!(app.output.dir, Some(PathBuf::from("/tmp/out")));
    }

    #[test]
    fn submit_empty_input_keeps_existing_output_dir() {
        let mut app = single_empty_layer_app();
        app.output.dir = Some(PathBuf::from("/existing"));
        update(&mut app, Action::EnterOutputDirInput);
        update(&mut app, Action::SubmitInput);
        assert_eq!(app.output.dir, Some(PathBuf::from("/existing")));
    }

    #[test]
    fn cancel_input_exits_without_changing_output_dir() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::EnterOutputDirInput);
        update(&mut app, Action::InputChar('/'));
        update(&mut app, Action::CancelInput);
        assert!(!app.modal.active);
        assert!(app.modal.buffer.is_empty());
        assert_eq!(app.output.dir, None);
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
        assert_eq!(app.nav.layer_index, 1);
        update(&mut app, Action::NavigateLayer { delta: 10 });
        assert_eq!(app.nav.layer_index, 2);
        update(&mut app, Action::NavigateLayer { delta: -100 });
        assert_eq!(app.nav.layer_index, 0);
    }

    #[test]
    fn navigate_layer_clears_selection_and_resets_detail_scroll() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"])), layer(1, empty_tree())]);
        app.selection
            .toggle_file("/a", &app.nav.cached_tree.clone());
        app.nav.detail_scroll = 5;
        update(&mut app, Action::NavigateLayer { delta: 1 });
        assert_eq!(app.nav.layer_index, 1);
        assert!(app.selection.is_empty());
        assert_eq!(app.nav.detail_scroll, 0);
    }

    // ── File navigation ──────────────────────────────────────────────────

    #[test]
    fn navigate_file_clamps_to_row_count() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a", "/b", "/c"]))]);
        // file_rows has 3 entries
        update(&mut app, Action::NavigateFile { delta: 1 });
        assert_eq!(app.nav.file_index, 1);
        update(&mut app, Action::NavigateFile { delta: 100 });
        assert_eq!(app.nav.file_index, app.nav.file_rows.len() - 1);
        update(&mut app, Action::NavigateFile { delta: -100 });
        assert_eq!(app.nav.file_index, 0);
    }

    #[test]
    fn navigate_file_with_no_rows_is_safe() {
        let mut app = single_empty_layer_app();
        assert!(app.nav.file_rows.is_empty());
        update(&mut app, Action::NavigateFile { delta: 1 });
        assert_eq!(app.nav.file_index, 0);
    }

    // ── Details scroll ────────────────────────────────────────────────────

    #[test]
    fn scroll_details_saturates() {
        let mut app = single_empty_layer_app();
        update(&mut app, Action::ScrollDetails { delta: 5 });
        assert_eq!(app.nav.detail_scroll, 5);
        update(&mut app, Action::ScrollDetails { delta: -2 });
        assert_eq!(app.nav.detail_scroll, 3);
        update(&mut app, Action::ScrollDetails { delta: -100 });
        assert_eq!(app.nav.detail_scroll, 0);
    }

    // ── Toggle expand ────────────────────────────────────────────────────

    #[test]
    fn toggle_expand_on_dir_toggles_membership() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/d/x"]))]);
        // file_rows[0] is /d (a dir, the only top-level row when /d is collapsed).
        let dir_path = app.nav.file_rows[0].path.clone();
        assert!(app.nav.file_rows[0].is_dir);
        update(&mut app, Action::ToggleExpand);
        assert!(app.nav.expanded_dirs.contains(&dir_path));
        update(&mut app, Action::ToggleExpand);
        assert!(!app.nav.expanded_dirs.contains(&dir_path));
    }

    #[test]
    fn toggle_expand_on_file_is_noop() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/a"]))]);
        // /a is a file, not a dir
        assert!(!app.nav.file_rows[0].is_dir);
        update(&mut app, Action::ToggleExpand);
        assert!(app.nav.expanded_dirs.is_empty());
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

    // ── Cache invalidation ────────────────────────────────────────────────

    #[test]
    fn cache_invalidates_on_navigate_layer() {
        let mut app = app_with_layers(vec![
            layer(0, tree_with(&["/layer0_file"])),
            layer(1, tree_with(&["/layer1_file"])),
        ]);
        // Start on layer 0; in layer-only mode, we see layer 0's files
        assert_eq!(app.nav.file_rows[0].path, "/layer0_file");
        // Navigate to layer 1; cache should be invalidated, tree rebuilt
        update(&mut app, Action::NavigateLayer { delta: 1 });
        assert_eq!(app.nav.layer_index, 1);
        assert_eq!(app.nav.file_rows[0].path, "/layer1_file");
    }

    #[test]
    fn cache_invalidates_on_toggle_cumulative() {
        let mut app = app_with_layers(vec![
            layer(0, tree_with(&["/a"])),
            layer(1, tree_with(&["/b"])),
        ]);
        // Start on layer 1, non-cumulative: only /b is visible
        update(&mut app, Action::NavigateLayer { delta: 1 });
        let non_cumulative_rows: Vec<String> =
            app.nav.file_rows.iter().map(|r| r.path.clone()).collect();
        assert_eq!(non_cumulative_rows.len(), 1);
        assert_eq!(non_cumulative_rows[0], "/b");

        // Toggle to cumulative: both /a and /b should be visible
        update(&mut app, Action::ToggleCumulative);
        let cumulative_rows: Vec<String> =
            app.nav.file_rows.iter().map(|r| r.path.clone()).collect();
        assert_eq!(cumulative_rows.len(), 2);
        assert!(cumulative_rows.iter().any(|r| r == "/a"));
        assert!(cumulative_rows.iter().any(|r| r == "/b"));

        // Toggle back to non-cumulative: only /b again
        update(&mut app, Action::ToggleCumulative);
        let non_cumulative_again: Vec<String> =
            app.nav.file_rows.iter().map(|r| r.path.clone()).collect();
        assert_eq!(non_cumulative_rows, non_cumulative_again);
    }

    #[test]
    fn cache_skips_tree_rebuild_on_toggle_expand() {
        let mut app = app_with_layers(vec![layer(0, tree_with(&["/d/x", "/d/y", "/d/z"]))]);
        // File rows should have /d collapsed initially
        assert_eq!(app.nav.file_rows.len(), 1);
        assert_eq!(app.nav.file_rows[0].path, "/d");
        assert!(!app.nav.file_rows[0].expanded);

        // Expand /d: tree stays the same, rows re-flatten to show children
        let dir_path = app.nav.file_rows[0].path.clone();
        update(&mut app, Action::ToggleExpand);
        assert!(app.nav.expanded_dirs.contains(&dir_path));
        // After expansion, we should see /d plus its children
        assert_eq!(app.nav.file_rows.len(), 4); // /d, /d/x, /d/y, /d/z
        assert_eq!(app.nav.file_rows[0].path, "/d");
        assert!(app.nav.file_rows[0].expanded);

        // Collapse /d again: tree still unchanged, rows re-flatten
        update(&mut app, Action::ToggleExpand);
        assert!(!app.nav.expanded_dirs.contains(&dir_path));
        // Back to just /d (collapsed)
        assert_eq!(app.nav.file_rows.len(), 1);
        assert_eq!(app.nav.file_rows[0].path, "/d");
        assert!(!app.nav.file_rows[0].expanded);
    }

    #[test]
    fn cache_handles_layer_navigation_then_expand() {
        // This test verifies that toggling expand after navigating layers
        // produces correct rows (both cache invalidation scenarios combined).
        let mut app = app_with_layers(vec![
            layer(0, tree_with(&["/a/b", "/a/c"])),
            layer(1, tree_with(&["/d/e", "/d/f"])),
        ]);
        // Start on layer 0
        assert!(app.nav.file_rows[0].path.starts_with("/a"));

        // Navigate to layer 1
        update(&mut app, Action::NavigateLayer { delta: 1 });
        let dir_path = app.nav.file_rows[0].path.clone();
        assert!(dir_path.starts_with("/d"));

        // Expand /d
        update(&mut app, Action::ToggleExpand);
        assert!(app.nav.file_rows.len() > 1);
        assert_eq!(app.nav.file_rows[0].path, "/d");

        // Collapse /d
        update(&mut app, Action::ToggleExpand);
        assert_eq!(app.nav.file_rows.len(), 1);
        assert_eq!(app.nav.file_rows[0].path, "/d");
    }
}
