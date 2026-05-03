use crossterm::event::KeyCode;

use crate::ui::Pane;

/// A user-initiated action, parsed from a key event in some context.
/// Pure data — produced by [`key_to_action`], consumed by [`crate::update::update`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    CyclePaneFocus,
    ToggleCumulative,
    CycleOutputFormat,
    EnterOutputDirInput,

    // Modal input lifecycle
    SubmitInput,
    CancelInput,
    InputBackspace,
    InputChar(char),

    // Navigation — focus baked in at parse time so the update path doesn't
    // need to re-branch on focus.
    NavigateLayer { delta: i32 },
    NavigateFile { delta: i32 },
    ScrollDetails { delta: i32 },

    // Files-pane interactions
    ToggleExpand,
    ToggleSelection,

    // Extraction effects
    ExtractCurrentLayer,
    ExtractAllLayers,
    ExtractFiles,
}

/// Map a key event to an action, given the minimal app context needed:
/// modal-input mode and current pane focus. Pure — easy to test.
pub fn key_to_action(input_mode: bool, focus: Pane, key: KeyCode) -> Option<Action> {
    if input_mode {
        return match key {
            KeyCode::Enter => Some(Action::SubmitInput),
            KeyCode::Esc => Some(Action::CancelInput),
            KeyCode::Backspace => Some(Action::InputBackspace),
            KeyCode::Char(c) => Some(Action::InputChar(c)),
            _ => None,
        };
    }

    match key {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Tab => Some(Action::CyclePaneFocus),
        KeyCode::Char('t') => Some(Action::ToggleCumulative),
        KeyCode::Char('f') => Some(Action::CycleOutputFormat),
        KeyCode::Char('o') => Some(Action::EnterOutputDirInput),

        KeyCode::Char('j') | KeyCode::Down => Some(navigate(focus, 1)),
        KeyCode::Char('k') | KeyCode::Up => Some(navigate(focus, -1)),

        KeyCode::Enter if focus == Pane::Files => Some(Action::ToggleExpand),
        KeyCode::Char(' ') if focus == Pane::Files => Some(Action::ToggleSelection),

        KeyCode::Char('a') if focus == Pane::Layers => Some(Action::ExtractAllLayers),
        KeyCode::Char('e') => match focus {
            Pane::Layers => Some(Action::ExtractCurrentLayer),
            Pane::Files => Some(Action::ExtractFiles),
            Pane::Details => None,
        },

        _ => None,
    }
}

fn navigate(focus: Pane, delta: i32) -> Action {
    match focus {
        Pane::Layers => Action::NavigateLayer { delta },
        Pane::Files => Action::NavigateFile { delta },
        Pane::Details => Action::ScrollDetails { delta },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;

    fn k(c: char) -> KeyCode {
        KeyCode::Char(c)
    }

    // ── Normal mode (input_mode = false) ──────────────────────────────────

    #[test]
    fn quit_on_q() {
        assert_eq!(
            key_to_action(false, Pane::Layers, k('q')),
            Some(Action::Quit)
        );
        assert_eq!(
            key_to_action(false, Pane::Files, k('q')),
            Some(Action::Quit)
        );
        assert_eq!(
            key_to_action(false, Pane::Details, k('q')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn tab_cycles_focus_in_any_pane() {
        for focus in [Pane::Layers, Pane::Files, Pane::Details] {
            assert_eq!(
                key_to_action(false, focus, KeyCode::Tab),
                Some(Action::CyclePaneFocus)
            );
        }
    }

    #[test]
    fn t_toggles_cumulative() {
        assert_eq!(
            key_to_action(false, Pane::Layers, k('t')),
            Some(Action::ToggleCumulative)
        );
    }

    #[test]
    fn f_cycles_format() {
        assert_eq!(
            key_to_action(false, Pane::Layers, k('f')),
            Some(Action::CycleOutputFormat)
        );
    }

    #[test]
    fn o_enters_output_dir_input() {
        assert_eq!(
            key_to_action(false, Pane::Layers, k('o')),
            Some(Action::EnterOutputDirInput)
        );
    }

    // ── Navigation ────────────────────────────────────────────────────────

    #[test]
    fn j_navigates_by_focus() {
        assert_eq!(
            key_to_action(false, Pane::Layers, k('j')),
            Some(Action::NavigateLayer { delta: 1 })
        );
        assert_eq!(
            key_to_action(false, Pane::Files, k('j')),
            Some(Action::NavigateFile { delta: 1 })
        );
        assert_eq!(
            key_to_action(false, Pane::Details, k('j')),
            Some(Action::ScrollDetails { delta: 1 })
        );
    }

    #[test]
    fn k_navigates_negatively() {
        assert_eq!(
            key_to_action(false, Pane::Layers, k('k')),
            Some(Action::NavigateLayer { delta: -1 })
        );
    }

    #[test]
    fn arrow_keys_match_jk() {
        assert_eq!(
            key_to_action(false, Pane::Files, KeyCode::Down),
            Some(Action::NavigateFile { delta: 1 })
        );
        assert_eq!(
            key_to_action(false, Pane::Files, KeyCode::Up),
            Some(Action::NavigateFile { delta: -1 })
        );
    }

    // ── Files-pane interactions ────────────────────────────────────────────

    #[test]
    fn enter_toggles_expand_only_in_files() {
        assert_eq!(
            key_to_action(false, Pane::Files, KeyCode::Enter),
            Some(Action::ToggleExpand)
        );
        assert_eq!(key_to_action(false, Pane::Layers, KeyCode::Enter), None);
        assert_eq!(key_to_action(false, Pane::Details, KeyCode::Enter), None);
    }

    #[test]
    fn space_toggles_selection_only_in_files() {
        assert_eq!(
            key_to_action(false, Pane::Files, k(' ')),
            Some(Action::ToggleSelection)
        );
        assert_eq!(key_to_action(false, Pane::Layers, k(' ')), None);
        assert_eq!(key_to_action(false, Pane::Details, k(' ')), None);
    }

    // ── Extraction ────────────────────────────────────────────────────────

    #[test]
    fn e_dispatches_by_focus() {
        assert_eq!(
            key_to_action(false, Pane::Layers, k('e')),
            Some(Action::ExtractCurrentLayer)
        );
        assert_eq!(
            key_to_action(false, Pane::Files, k('e')),
            Some(Action::ExtractFiles)
        );
        assert_eq!(key_to_action(false, Pane::Details, k('e')), None);
    }

    #[test]
    fn a_extracts_all_only_in_layers() {
        assert_eq!(
            key_to_action(false, Pane::Layers, k('a')),
            Some(Action::ExtractAllLayers)
        );
        assert_eq!(key_to_action(false, Pane::Files, k('a')), None);
        assert_eq!(key_to_action(false, Pane::Details, k('a')), None);
    }

    // ── Modal input mode ──────────────────────────────────────────────────

    #[test]
    fn input_mode_enter_submits() {
        assert_eq!(
            key_to_action(true, Pane::Layers, KeyCode::Enter),
            Some(Action::SubmitInput)
        );
    }

    #[test]
    fn input_mode_esc_cancels() {
        assert_eq!(
            key_to_action(true, Pane::Layers, KeyCode::Esc),
            Some(Action::CancelInput)
        );
    }

    #[test]
    fn input_mode_backspace() {
        assert_eq!(
            key_to_action(true, Pane::Layers, KeyCode::Backspace),
            Some(Action::InputBackspace)
        );
    }

    #[test]
    fn input_mode_char_inserts() {
        assert_eq!(
            key_to_action(true, Pane::Layers, k('x')),
            Some(Action::InputChar('x'))
        );
        // Even keys that are normally commands ('q') become text input.
        assert_eq!(
            key_to_action(true, Pane::Layers, k('q')),
            Some(Action::InputChar('q'))
        );
    }

    #[test]
    fn input_mode_ignores_other_keys() {
        assert_eq!(key_to_action(true, Pane::Layers, KeyCode::Tab), None);
        assert_eq!(key_to_action(true, Pane::Layers, KeyCode::Up), None);
    }

    #[test]
    fn unknown_keys_yield_none() {
        assert_eq!(
            key_to_action(false, Pane::Layers, KeyCode::Tab),
            Some(Action::CyclePaneFocus)
        );
        assert_eq!(key_to_action(false, Pane::Layers, KeyCode::F(1)), None);
    }
}
