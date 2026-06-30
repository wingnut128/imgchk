# --report Non-Interactive Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `--report` CLI flag that prints a JSON analysis of an image (layer summaries + suspicious-file findings) to stdout instead of launching the TUI.

**Architecture:** A new pure module `src/report.rs` transforms the existing `image::ImageInfo`/`tree::FileTree` data (already built before the TUI starts) into serializable report structs, including a per-layer suspicious-file scan over file permission bits and filename patterns. `src/main.rs` branches on the new `--report` flag right after image loading, before any terminal/TUI setup.

**Tech Stack:** Rust 2021, `serde`/`serde_json` (already a dependency), `clap` for the new flag, `cargo test` for unit tests.

## Global Constraints

- Spec source of truth: `docs/superpowers/specs/2026-06-30-report-mode-design.md`.
- No exit-code gating in this change — report is JSON-only, fetch errors behave exactly as they do today.
- No signature verification — `signature` field is always `null`, reserved for a future spec.
- No full file-tree/path dump in the report — summary counts + suspicious-file findings only.
- `command` field must be the full untruncated command (`command_format::clean_command`), not the TUI's `truncate_command`.
- Suspicious-file scan runs per-layer against that layer's own `FileTree`, not the cumulative/merged view.
- Directories and symlinks are never flagged as suspicious — rules apply to regular files only.
- A file may match more than one rule; emit one finding per matching rule.

---

### Task 1: `report` module — data structs and `build_report`

**Files:**
- Create: `src/report.rs`
- Modify: `src/main.rs:1-9` (add `mod report;` alongside the other `mod` declarations, alphabetically after `image`)

**Interfaces:**
- Consumes: `crate::image::{ImageInfo, LayerInfo}` (fields: `index: usize`, `digest: String`, `diff_id: String`, `size: u64`, `command: String`, `created: String`, `file_tree: tree::FileTree`; `ImageInfo` fields: `layers: Vec<LayerInfo>`, `total_size: u64`, `architecture: String`, `os: String`, `source: String`); `crate::command_format::clean_command(cmd: &str) -> String`; `crate::tree::{FileTree, FileNode}` (fields per `src/tree.rs`: `name`, `path`, `size`, `mode`, `is_dir`, `is_whiteout`, `is_opaque`, `link_target`, `children: BTreeMap<String, FileNode>`; method `insert_node(&mut self, path: &str, node: FileNode)` is `pub(crate)`, usable from `report` tests).
- Produces (for Task 2 / `main.rs`): `pub struct ReportImage` (Serialize), `pub fn build_report(image: &ImageInfo) -> ReportImage`.

- [ ] **Step 1: Write failing tests for the report structs and `build_report`**

Create `src/report.rs` with:

```rust
use serde::Serialize;

use crate::command_format::clean_command;
use crate::image::ImageInfo;
use crate::tree::{FileNode, FileTree};

#[derive(Serialize)]
pub struct ReportImage {
    pub source: String,
    pub architecture: String,
    pub os: String,
    pub total_size: u64,
    pub signature: Option<()>,
    pub layers: Vec<ReportLayer>,
}

#[derive(Serialize)]
pub struct ReportLayer {
    pub index: usize,
    pub digest: String,
    pub diff_id: String,
    pub size: u64,
    pub command: String,
    pub created: String,
    pub file_count: usize,
    pub suspicious_files: Vec<SuspiciousFile>,
}

#[derive(Serialize)]
pub struct SuspiciousFile {
    pub path: String,
    pub reason: &'static str,
    pub mode: Option<u32>,
}

pub fn build_report(image: &ImageInfo) -> ReportImage {
    ReportImage {
        source: image.source.clone(),
        architecture: image.architecture.clone(),
        os: image.os.clone(),
        total_size: image.total_size,
        signature: None,
        layers: image
            .layers
            .iter()
            .map(|layer| ReportLayer {
                index: layer.index,
                digest: layer.digest.clone(),
                diff_id: layer.diff_id.clone(),
                size: layer.size,
                command: clean_command(&layer.command),
                created: layer.created.clone(),
                file_count: layer.file_tree.file_count,
                suspicious_files: scan_suspicious(&layer.file_tree),
            })
            .collect(),
    }
}

pub fn scan_suspicious(_tree: &FileTree) -> Vec<SuspiciousFile> {
    Vec::new() // implemented in Task 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::LayerInfo;
    use std::path::PathBuf;

    fn empty_layer(index: usize, command: &str) -> LayerInfo {
        LayerInfo {
            index,
            digest: format!("sha256:digest{index}"),
            diff_id: format!("sha256:diffid{index}"),
            size: 1000 + index as u64,
            command: command.to_string(),
            created: "2026-01-01T00:00:00Z".to_string(),
            file_tree: FileTree::new(),
            blob_path: PathBuf::from("/tmp/blob"),
            media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
        }
    }

    #[test]
    fn build_report_maps_image_and_layer_fields() {
        let image = ImageInfo {
            layers: vec![empty_layer(0, "RUN   apt-get update")],
            total_size: 1000,
            architecture: "amd64".to_string(),
            os: "linux".to_string(),
            source: "nginx:latest".to_string(),
        };

        let report = build_report(&image);

        assert_eq!(report.source, "nginx:latest");
        assert_eq!(report.architecture, "amd64");
        assert_eq!(report.os, "linux");
        assert_eq!(report.total_size, 1000);
        assert!(report.signature.is_none());
        assert_eq!(report.layers.len(), 1);
        assert_eq!(report.layers[0].index, 0);
        assert_eq!(report.layers[0].digest, "sha256:digest0");
        assert_eq!(report.layers[0].command, "RUN apt-get update");
        assert_eq!(report.layers[0].file_count, 0);
    }

    #[test]
    fn build_report_serializes_signature_as_null() {
        let image = ImageInfo {
            layers: vec![],
            total_size: 0,
            architecture: "amd64".to_string(),
            os: "linux".to_string(),
            source: "alpine:3.19".to_string(),
        };

        let json = serde_json::to_string(&build_report(&image)).unwrap();
        assert!(json.contains("\"signature\":null"));
    }
}
```

- [ ] **Step 2: Run tests to confirm they pass (struct/mapping logic is already correct as written)**

Run: `cargo test report::tests -- --nocapture`
Expected: both tests pass. (If `clean_command` collapses whitespace differently than assumed, adjust the assertion in `build_report_maps_image_and_layer_fields` to match its actual output — check `src/command_format.rs:55` for the exact behavior before adjusting.)

- [ ] **Step 3: Wire the module into `main.rs`**

In `src/main.rs`, change:

```rust
mod action;
mod command_format;
mod extract;
mod image;
mod selection;
mod tree;
mod ui;
mod update;
mod view;
```

to:

```rust
mod action;
mod command_format;
mod extract;
mod image;
mod report;
mod selection;
mod tree;
mod ui;
mod update;
mod view;
```

- [ ] **Step 4: Build to confirm the module compiles and is wired in**

Run: `cargo build`
Expected: builds with no errors (a `dead_code` warning for `build_report`/`scan_suspicious` being unused is expected at this point — it's resolved in Task 3 when `main.rs` calls them).

- [ ] **Step 5: Commit**

```bash
git add src/report.rs src/main.rs
git commit -m "feat: add report module with ReportImage/ReportLayer structs"
```

---

### Task 2: Suspicious-file scan rules

**Files:**
- Modify: `src/report.rs` (replace the `scan_suspicious` stub from Task 1)

**Interfaces:**
- Consumes: `crate::tree::{FileTree, FileNode}` from Task 1's imports; `SuspiciousFile` struct from Task 1.
- Produces: `pub fn scan_suspicious(tree: &FileTree) -> Vec<SuspiciousFile>` (signature unchanged from Task 1, now fully implemented). No other task depends on internals beyond this signature.

- [ ] **Step 1: Write failing tests for each suspicious-file rule**

Add to the `tests` module in `src/report.rs` (keep existing tests from Task 1):

```rust
    fn file_node(path: &str, mode: u32) -> FileNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        FileNode {
            name,
            path: path.to_string(),
            size: 100,
            mode,
            is_dir: false,
            is_whiteout: false,
            is_opaque: false,
            link_target: None,
            children: std::collections::BTreeMap::new(),
        }
    }

    fn dir_node(path: &str, mode: u32) -> FileNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        FileNode {
            name,
            path: path.to_string(),
            size: 0,
            mode,
            is_dir: true,
            is_whiteout: false,
            is_opaque: false,
            link_target: None,
            children: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn scan_suspicious_flags_setuid_file() {
        let mut tree = FileTree::new();
        tree.insert_node("/usr/bin/sudo", file_node("/usr/bin/sudo", 0o104755));

        let findings = scan_suspicious(&tree);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].path, "/usr/bin/sudo");
        assert_eq!(findings[0].reason, "setuid");
        assert_eq!(findings[0].mode, Some(0o104755));
    }

    #[test]
    fn scan_suspicious_flags_setgid_file() {
        let mut tree = FileTree::new();
        tree.insert_node("/usr/bin/wall", file_node("/usr/bin/wall", 0o102755));

        let findings = scan_suspicious(&tree);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].reason, "setgid");
    }

    #[test]
    fn scan_suspicious_flags_world_writable_file() {
        let mut tree = FileTree::new();
        tree.insert_node("/tmp/scratch", file_node("/tmp/scratch", 0o100666));

        let findings = scan_suspicious(&tree);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].reason, "world_writable");
    }

    #[test]
    fn scan_suspicious_does_not_flag_directory_with_suspicious_mode_bits() {
        let mut tree = FileTree::new();
        tree.insert_node("/tmp", dir_node("/tmp", 0o104777));

        let findings = scan_suspicious(&tree);

        assert!(findings.is_empty());
    }

    #[test]
    fn scan_suspicious_flags_secret_pattern_filenames() {
        let mut tree = FileTree::new();
        tree.insert_node("/root/.ssh/id_rsa", file_node("/root/.ssh/id_rsa", 0o100600));
        tree.insert_node("/etc/tls/server.pem", file_node("/etc/tls/server.pem", 0o100644));
        tree.insert_node("/app/.env", file_node("/app/.env", 0o100644));
        tree.insert_node("/app/keys.txt", file_node("/app/keys.txt", 0o100644));

        let findings = scan_suspicious(&tree);
        let secret_paths: Vec<&str> = findings
            .iter()
            .filter(|f| f.reason == "secret_pattern")
            .map(|f| f.path.as_str())
            .collect();

        assert!(secret_paths.contains(&"/root/.ssh/id_rsa"));
        assert!(secret_paths.contains(&"/etc/tls/server.pem"));
        assert!(secret_paths.contains(&"/app/.env"));
        assert!(!secret_paths.contains(&"/app/keys.txt"));
    }

    #[test]
    fn scan_suspicious_emits_two_findings_for_file_matching_two_rules() {
        let mut tree = FileTree::new();
        tree.insert_node(
            "/etc/secrets/server.key",
            file_node("/etc/secrets/server.key", 0o104666),
        );

        let findings = scan_suspicious(&tree);
        let reasons: Vec<&str> = findings.iter().map(|f| f.reason).collect();

        assert_eq!(findings.len(), 2);
        assert!(reasons.contains(&"setuid"));
        assert!(reasons.contains(&"secret_pattern"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test report::tests::scan_suspicious -- --nocapture`
Expected: FAIL — all `scan_suspicious_*` tests fail because the stub returns an empty `Vec`.

- [ ] **Step 3: Implement `scan_suspicious`**

Replace the stub in `src/report.rs`:

```rust
const SECRET_EXACT_NAMES: &[&str] = &["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519", ".env"];
const SECRET_EXTENSIONS: &[&str] = &[".pem", ".key", ".p12"];

pub fn scan_suspicious(tree: &FileTree) -> Vec<SuspiciousFile> {
    let mut findings = Vec::new();
    walk(&tree.root, &mut findings);
    findings
}

fn walk(node: &FileNode, findings: &mut Vec<SuspiciousFile>) {
    if node.is_dir {
        for child in node.children.values() {
            walk(child, findings);
        }
        return;
    }

    if node.mode & 0o4000 != 0 {
        findings.push(SuspiciousFile {
            path: node.path.clone(),
            reason: "setuid",
            mode: Some(node.mode),
        });
    }
    if node.mode & 0o2000 != 0 {
        findings.push(SuspiciousFile {
            path: node.path.clone(),
            reason: "setgid",
            mode: Some(node.mode),
        });
    }
    if node.mode & 0o002 != 0 {
        findings.push(SuspiciousFile {
            path: node.path.clone(),
            reason: "world_writable",
            mode: Some(node.mode),
        });
    }
    if is_secret_pattern(&node.name) {
        findings.push(SuspiciousFile {
            path: node.path.clone(),
            reason: "secret_pattern",
            mode: None,
        });
    }
}

fn is_secret_pattern(name: &str) -> bool {
    SECRET_EXACT_NAMES.contains(&name) || SECRET_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test report:: -- --nocapture`
Expected: all tests in `report` module (Task 1 + Task 2) pass.

- [ ] **Step 5: Commit**

```bash
git add src/report.rs
git commit -m "feat: implement suspicious-file scan rules in report module"
```

---

### Task 3: `--report` CLI flag and `main.rs` wiring

**Files:**
- Modify: `src/main.rs:53-90` (struct `Cli` and `fn main`)

**Interfaces:**
- Consumes: `report::build_report(image: &ImageInfo) -> ReportImage` (Task 1), `ReportImage: Serialize` (Task 1), `serde_json::to_string_pretty`.
- Produces: nothing further consumed by other tasks — this is the final integration point.

- [ ] **Step 1: Add the `--report` flag to `Cli`**

In `src/main.rs`, change the `Cli` struct (currently ending at line 70):

```rust
struct Cli {
    /// Image reference (e.g., nginx:latest) or path to a tarball
    image: Option<String>,

    /// Output directory for extracted files/layers
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Target platform (e.g., linux/amd64, linux/arm64)
    #[arg(long, default_value = "linux/amd64")]
    platform: String,
}
```

to:

```rust
struct Cli {
    /// Image reference (e.g., nginx:latest) or path to a tarball
    image: Option<String>,

    /// Output directory for extracted files/layers
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Target platform (e.g., linux/amd64, linux/arm64)
    #[arg(long, default_value = "linux/amd64")]
    platform: String,

    /// Print a JSON analysis report to stdout instead of launching the TUI
    #[arg(long)]
    report: bool,
}
```

- [ ] **Step 2: Branch on `cli.report` right after image loading**

In `fn main`, after the existing block that loads `image` (the `let image = if image::is_tarball(...) { ... } else { ... };` block, ending around line 90), insert:

```rust
    if cli.report {
        let report = report::build_report(&image);
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
```

before whatever TUI/`App`/terminal setup code currently follows.

- [ ] **Step 3: Update the CLI help text to document the new flag**

In `src/main.rs`, the `EXAMPLES_HELP` const currently includes an `ENVIRONMENT:` section. Add a usage example by changing:

```rust
    Set an output directory for extractions:
        imgchk -o /tmp/extracted ghcr.io/org/app:v1.2

TUI KEYBINDINGS:
```

to:

```rust
    Set an output directory for extractions:
        imgchk -o /tmp/extracted ghcr.io/org/app:v1.2

    Print a JSON report instead of launching the TUI:
        imgchk nginx:latest --report

TUI KEYBINDINGS:
```

- [ ] **Step 4: Build and manually verify report output on a local tarball**

Run: `cargo build`
Expected: builds with no errors or warnings (the earlier `dead_code` warning from Task 1 is now resolved since `main.rs` calls `report::build_report`).

Run (using any tarball available in the repo's test fixtures, or build one):
```bash
docker save alpine:3.19 -o /tmp/alpine.tar 2>/dev/null || echo "skip if docker unavailable"
cargo run -- /tmp/alpine.tar --report | head -30
```
Expected: pretty-printed JSON starting with `{` and containing `"signature": null` and a `"layers"` array. If Docker isn't available locally, skip the manual run and rely on Step 5's automated test instead — this step is a sanity check, not a blocking gate.

- [ ] **Step 5: Add an integration-style test for the report branch**

Add a `#[cfg(test)]` module at the bottom of `src/main.rs` (create one if none exists — check first, since `main.rs` may not currently have one):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_report_flag() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest", "--report"]);
        assert!(cli.report);
        assert_eq!(cli.image.as_deref(), Some("nginx:latest"));
    }

    #[test]
    fn cli_report_defaults_to_false() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest"]);
        assert!(!cli.report);
    }
}
```

- [ ] **Step 6: Run the new tests to verify they pass**

Run: `cargo test cli_ -- --nocapture`
Expected: both `cli_parses_report_flag` and `cli_report_defaults_to_false` pass.

- [ ] **Step 7: Run the full test suite to confirm no regressions**

Run: `cargo test`
Expected: all tests pass (existing suite + new `report::tests::*` + `tests::cli_*`).

- [ ] **Step 8: Run fmt and clippy (matches the project's pre-commit hook)**

Run: `cargo fmt --check && cargo clippy -- -D warnings`
Expected: no formatting diffs, no clippy warnings. If `cargo fmt` reports diffs, run `cargo fmt` (no `--check`) to apply them before committing.

- [ ] **Step 9: Commit**

```bash
git add src/main.rs
git commit -m "feat: add --report flag for non-interactive JSON output"
```

---

## Post-plan verification

After Task 3, run `just test` (or `cargo test`) once more from a clean state to confirm the full suite is green, and skim the printed JSON from Step 4's manual run against the schema in the spec (`docs/superpowers/specs/2026-06-30-report-mode-design.md`) to confirm field names match exactly.
