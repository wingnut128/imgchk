/// A tar-entry path that has been validated as safe to extract under an
/// output directory.
///
/// Holds two forms of the same logical path:
/// - `absolute`: leading-slash form (`/foo/bar`) — keys against
///   tree-derived selection sets.
/// - `relative`: relative form (`foo/bar`) — safe to join with an
///   output-dir path.
///
/// Construct only via [`safe_path`], which enforces the safety invariants
/// (no `..` components, no absolute-path escapes, no Windows drive
/// letters, no embedded backslash separators).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafePath {
    pub absolute: String,
    pub relative: String,
}

/// Validate and normalize a raw tar-entry path. Returns `None` for paths
/// that would escape the extraction root once joined.
///
/// Behavior:
/// - Strips leading `./`, `/`, and `\` separators.
/// - Drops `.` and `..` components (doesn't pop — keeps siblings under root).
/// - Splits on both `/` and `\` to defend against Windows-style separators
///   embedded in tar archives.
/// - Returns `None` for empty, root-only, or drive-letter-prefixed inputs.
pub fn safe_path(raw: &str) -> Option<SafePath> {
    let parts: Vec<&str> = raw
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .collect();

    if parts.is_empty() {
        return None;
    }

    // Reject Windows drive letters like "C:" appearing as the first
    // component — these would resolve absolutely on Windows.
    if parts[0].len() == 2
        && parts[0].chars().nth(1) == Some(':')
        && parts[0]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
    {
        return None;
    }

    let relative = parts.join("/");
    let absolute = format!("/{relative}");
    Some(SafePath { absolute, relative })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(raw: &str) -> Option<String> {
        safe_path(raw).map(|p| p.relative)
    }

    fn abs(raw: &str) -> Option<String> {
        safe_path(raw).map(|p| p.absolute)
    }

    #[test]
    fn plain_relative_path_is_kept() {
        assert_eq!(rel("foo/bar"), Some("foo/bar".into()));
        assert_eq!(abs("foo/bar"), Some("/foo/bar".into()));
    }

    #[test]
    fn leading_slash_is_stripped() {
        assert_eq!(rel("/etc/hosts"), Some("etc/hosts".into()));
        assert_eq!(abs("/etc/hosts"), Some("/etc/hosts".into()));
    }

    #[test]
    fn leading_dot_slash_is_stripped() {
        assert_eq!(rel("./foo"), Some("foo".into()));
    }

    #[test]
    fn parent_traversal_attempt_does_not_escape() {
        // `..` components are *dropped*, not popped — siblings of the
        // intended file are left under the output root.
        assert_eq!(rel("../etc/passwd"), Some("etc/passwd".into()));
        assert_eq!(rel("/foo/../../etc"), Some("foo/etc".into()));
    }

    #[test]
    fn embedded_dot_components_collapse() {
        assert_eq!(rel("foo/./bar"), Some("foo/bar".into()));
    }

    #[test]
    fn double_slashes_collapse() {
        assert_eq!(rel("foo//bar"), Some("foo/bar".into()));
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(safe_path(""), None);
    }

    #[test]
    fn root_only_is_rejected() {
        assert_eq!(safe_path("/"), None);
        assert_eq!(safe_path("//"), None);
    }

    #[test]
    fn dotdot_only_is_rejected() {
        assert_eq!(safe_path(".."), None);
        assert_eq!(safe_path("../.."), None);
    }

    #[test]
    fn dot_only_is_rejected() {
        assert_eq!(safe_path("."), None);
        assert_eq!(safe_path("./"), None);
    }

    #[test]
    fn backslash_separators_are_normalized() {
        assert_eq!(rel("foo\\bar"), Some("foo/bar".into()));
        assert_eq!(rel("foo\\..\\bar"), Some("foo/bar".into()));
    }

    #[test]
    fn windows_drive_letter_is_rejected() {
        assert_eq!(safe_path("C:\\foo"), None);
        assert_eq!(safe_path("d:/etc"), None);
    }

    #[test]
    fn colon_in_filename_is_kept() {
        // Colons are legal in many filenames; only reject the specific
        // 2-char `<letter>:` drive-letter pattern as the first component.
        assert_eq!(rel("foo:bar"), Some("foo:bar".into()));
        assert_eq!(rel("ab:/etc"), Some("ab:/etc".into()));
    }

    #[test]
    fn deeply_nested_path_is_kept() {
        assert_eq!(
            rel("usr/local/share/man/man1/foo.1"),
            Some("usr/local/share/man/man1/foo.1".into()),
        );
    }
}
