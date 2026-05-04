/// Split a shell command at separators (`;`, `&&`, `||`, `|`) into lines.
/// Each line keeps its trailing separator. Empty input yields a single
/// line containing the original string.
pub fn format_command(cmd: &str) -> Vec<String> {
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

/// Strip Docker-history noise (`/bin/sh -c`, `#(nop)`) and collapse runs
/// of whitespace into single spaces.
pub fn clean_command(cmd: &str) -> String {
    let stripped = cmd.replace("/bin/sh -c ", "").replace("#(nop) ", "");
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Clean a command and truncate to `max_len` characters with an ellipsis.
pub fn truncate_command(cmd: &str, max_len: usize) -> String {
    let cleaned = clean_command(cmd);
    if cleaned.len() > max_len {
        format!("{}…", &cleaned[..max_len.saturating_sub(1)])
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_command ─────────────────────────────────────────────────────

    #[test]
    fn format_splits_on_semicolon() {
        assert_eq!(
            format_command("apk update; apk add curl"),
            vec!["apk update;".to_string(), "apk add curl".to_string()]
        );
    }

    #[test]
    fn format_splits_on_double_amp() {
        assert_eq!(
            format_command("foo && bar"),
            vec!["foo &&".to_string(), "bar".to_string()]
        );
    }

    #[test]
    fn format_splits_on_double_pipe() {
        assert_eq!(
            format_command("foo || bar"),
            vec!["foo ||".to_string(), "bar".to_string()]
        );
    }

    #[test]
    fn format_splits_on_single_pipe() {
        assert_eq!(
            format_command("ls | grep foo"),
            vec!["ls |".to_string(), "grep foo".to_string()]
        );
    }

    #[test]
    fn format_distinguishes_pipe_from_double_pipe() {
        // `||` should not be treated as two `|`s.
        let result = format_command("a || b | c");
        assert_eq!(
            result,
            vec!["a ||".to_string(), "b |".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn format_returns_original_when_no_separators() {
        assert_eq!(format_command("echo hello"), vec!["echo hello".to_string()]);
    }

    #[test]
    fn format_handles_empty_input() {
        assert_eq!(format_command(""), vec!["".to_string()]);
    }

    // ── clean_command ──────────────────────────────────────────────────────

    #[test]
    fn clean_strips_bin_sh_prefix() {
        assert_eq!(clean_command("/bin/sh -c echo hi"), "echo hi");
    }

    #[test]
    fn clean_strips_nop_marker() {
        assert_eq!(
            clean_command("#(nop) ENV PATH=/usr/bin"),
            "ENV PATH=/usr/bin"
        );
    }

    #[test]
    fn clean_collapses_whitespace() {
        assert_eq!(clean_command("foo  \n\tbar"), "foo bar");
    }

    #[test]
    fn clean_strips_both_prefixes() {
        assert_eq!(clean_command("/bin/sh -c #(nop) CMD echo"), "CMD echo");
    }

    // ── truncate_command ───────────────────────────────────────────────────

    #[test]
    fn truncate_returns_full_command_when_short() {
        assert_eq!(truncate_command("ls -la", 20), "ls -la");
    }

    #[test]
    fn truncate_adds_ellipsis_when_exceeding() {
        let result = truncate_command("0123456789abcdef", 10);
        assert!(result.ends_with('…'));
        assert!(result.chars().count() <= 10);
    }

    #[test]
    fn truncate_cleans_before_measuring() {
        // After cleaning, "echo hi" is 7 chars — well under 20.
        assert_eq!(truncate_command("/bin/sh -c echo hi", 20), "echo hi");
    }
}
