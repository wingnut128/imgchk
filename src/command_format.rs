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

/// Remove control characters (ESC, other C0/C1 codes, embedded newlines/tabs)
/// so untrusted image or scanner data can't inject terminal escape sequences
/// when the value is printed to a terminal. Callers add their own line
/// separators, so per-field stripping keeps layout intact and collapses any
/// injected newline into a single line.
pub fn strip_control(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Clean a command and truncate to `max_len` characters with an ellipsis.
/// Character count is based on Unicode scalar values (.chars()), not display width.
pub fn truncate_command(cmd: &str, max_len: usize) -> String {
    let cleaned = clean_command(cmd);
    let char_count = cleaned.chars().count();
    if char_count > max_len {
        let truncated: String = cleaned.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_control ──────────────────────────────────────────────────────

    #[test]
    fn strip_control_removes_escape_and_control_bytes() {
        // Untrusted data embedding ANSI escapes + a bell.
        let input = "pkg\u{1b}[31m\u{07}name";
        let out = strip_control(input);
        assert!(!out.contains('\u{1b}'));
        assert!(!out.contains('\u{07}'));
        assert_eq!(out, "pkg[31mname");
    }

    #[test]
    fn strip_control_removes_embedded_newlines_and_tabs() {
        assert_eq!(strip_control("a\nb\tc"), "abc");
    }

    #[test]
    fn strip_control_leaves_normal_text_untouched() {
        assert_eq!(strip_control("openssl 3.0.11"), "openssl 3.0.11");
    }

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

    #[test]
    fn truncate_handles_emoji_at_boundary() {
        // Emoji "🎉" is a single Unicode scalar value (one char)
        // but multiple bytes in UTF-8 (4 bytes: F0 9F 8E 89).
        // With max_len=5, we should take 4 chars + ellipsis, not panic.
        let input = "/bin/sh -c echo 🎉test";
        let result = truncate_command(input, 5);
        // After cleaning: "echo 🎉test" (10 chars)
        assert!(result.ends_with('…'));
        assert!(result.chars().count() <= 5);
        // Should not contain unfinished UTF-8 sequences
        assert!(result.is_char_boundary(result.len()));
    }

    #[test]
    fn truncate_handles_accented_chars_at_boundary() {
        // Accented char "é" is a single char but multiple bytes (C3 A9 in UTF-8).
        let input = "café";
        let result = truncate_command(input, 3);
        // "café" has 4 chars, max is 3, so truncate to 2 + ellipsis.
        assert_eq!(result, "ca…");
        assert_eq!(result.chars().count(), 3);
    }

    #[test]
    fn truncate_string_exactly_max_len() {
        // If string is exactly max_len chars, don't truncate.
        assert_eq!(truncate_command("hello", 5), "hello");
        assert_eq!(truncate_command("hello", 6), "hello");
    }

    #[test]
    fn truncate_with_max_len_of_1() {
        // When max_len is 1, we can only fit the ellipsis.
        let result = truncate_command("hello", 1);
        assert_eq!(result, "…");
        assert_eq!(result.chars().count(), 1);
    }

    #[test]
    fn truncate_with_max_len_of_2() {
        // With max_len=2, take 1 char + ellipsis.
        let result = truncate_command("hello", 2);
        assert_eq!(result, "h…");
        assert_eq!(result.chars().count(), 2);
    }

    #[test]
    fn truncate_ascii_regression() {
        // Ensure pure-ASCII input still works as before.
        let result = truncate_command("abcdefghij", 5);
        assert_eq!(result, "abcd…");
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn truncate_mixed_multibyte_and_ascii() {
        // String with mixed emoji and ASCII: "🎉ab🎉cdef" (8 chars total)
        let input = "🎉ab🎉cdef";
        let result = truncate_command(input, 6);
        // Truncate to 5 chars + ellipsis
        assert_eq!(result, "🎉ab🎉c…");
        assert_eq!(result.chars().count(), 6);
    }
}
