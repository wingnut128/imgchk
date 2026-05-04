use std::collections::HashSet;

/// Decides whether a tar entry — keyed by its absolute (leading-slash)
/// path — should be included in the output.
///
/// Pure: no I/O, no filesystem access. Implementors should be cheap to
/// query in a tight loop over tar entries.
pub trait FileSelector {
    fn matches(&self, absolute_path: &str) -> bool;
}

/// Includes only entries whose absolute path is in the configured set.
pub struct SelectedSet(HashSet<String>);

impl SelectedSet {
    pub fn new<I, S>(paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(paths.into_iter().map(Into::into).collect())
    }
}

impl FileSelector for SelectedSet {
    fn matches(&self, absolute_path: &str) -> bool {
        self.0.contains(absolute_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_set_matches_only_listed_paths() {
        let s = SelectedSet::new(["/etc/hosts", "/usr/bin/ls"]);
        assert!(s.matches("/etc/hosts"));
        assert!(s.matches("/usr/bin/ls"));
        assert!(!s.matches("/etc/passwd"));
    }

    #[test]
    fn selected_set_is_path_exact() {
        let s = SelectedSet::new(["/etc/hosts"]);
        // No prefix matching — directories are not implicit.
        assert!(!s.matches("/etc"));
        assert!(!s.matches("/etc/hosts/extra"));
    }

    #[test]
    fn empty_selected_set_matches_nothing() {
        let s = SelectedSet::new(Vec::<String>::new());
        assert!(!s.matches("/etc/hosts"));
    }
}
