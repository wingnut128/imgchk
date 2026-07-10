# `--dockerfile` Reconstruction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `--dockerfile` flag that prints an image's build steps as either a best-effort reconstructed Dockerfile or a raw ordered command list, and expose ordered `history` + reconstructed `dockerfile` fields in `--report`.

**Architecture:** A new full-history parser (`parse_full_history`) keeps empty-layer instructions the existing `parse_history` drops, surfaced as `ImageInfo.history`. A new pure module `src/dockerfile.rs` renders that history into a Dockerfile (`reconstruct`) or a verbatim list (`render_raw`). `main.rs` gains the flag + a standalone print branch; `report.rs` gains the two JSON fields.

**Tech Stack:** Rust 2021, serde/serde_json, clap. Tests are in-file `#[cfg(test)] mod tests`.

## Global Constraints

- Reconstruction is **history-only** — do NOT read final directives from the image config (`Config.Cmd`, `Config.Env`, ...).
- The reconstructed Dockerfile is **best-effort, not guaranteed-buildable**: emit NO `FROM` line (only an annotated comment); `COPY`/`ADD` legacy `dir:<hash> in <dest>` becomes an annotated `<context unavailable>` line.
- `parse_full_history` must PRESERVE ORDER and INCLUDE empty-layer entries. The existing `parse_history` stays unchanged (layer↔blob alignment depends on it).
- `history` + `dockerfile` are ALWAYS present in `--report` (additive keys). JSON field names exactly: `created_by`, `empty_layer`, `created` (in `history[]`), and top-level `history`, `dockerfile`.
- `regex` is NOT a dependency — use manual string parsing, no regex crate.
- No exit-code gating. `--dockerfile` + `--scan` without `--report` is rejected with a clear error.
- Pre-commit hook runs `cargo fmt --check` + `cargo clippy` (with `-D warnings`) — code must be clean before each commit.

---

### Task 1: Full-history data layer

**Files:**
- Modify: `src/image/mod.rs` (add `HistoryStep`, `parse_full_history`, `ImageInfo.history`)
- Modify: `src/image/registry.rs` (populate `history`)
- Modify: `src/image/tarball.rs` (populate `history` in both `ImageInfo` literals)
- Test: `src/image/mod.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub(crate) struct HistoryStep { pub created_by: String, pub empty_layer: bool, pub created: String }` (derives `Clone, Debug, serde::Serialize`); `pub(crate) fn parse_full_history(config: &ImageConfig) -> Vec<HistoryStep>`; `ImageInfo.history: Vec<HistoryStep>`.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `src/image/mod.rs` (it already exists just below `parse_history`):

```rust
    #[test]
    fn parse_full_history_keeps_empty_layers_and_order() {
        let config = ImageConfig {
            history: Some(vec![
                HistoryEntry {
                    created_by: Some("/bin/sh -c #(nop)  ENV A=1".into()),
                    created: Some("t0".into()),
                    empty_layer: Some(true),
                },
                HistoryEntry {
                    created_by: Some("/bin/sh -c apt-get update".into()),
                    created: Some("t1".into()),
                    empty_layer: Some(false),
                },
                HistoryEntry {
                    created_by: None,
                    created: None,
                    empty_layer: None,
                },
            ]),
            ..Default::default()
        };
        let steps = parse_full_history(&config);
        assert_eq!(steps.len(), 3);
        assert!(steps[0].empty_layer);
        assert_eq!(steps[0].created_by, "/bin/sh -c #(nop)  ENV A=1");
        assert_eq!(steps[1].empty_layer, false);
        assert_eq!(steps[1].created, "t1");
        // Missing fields default cleanly.
        assert_eq!(steps[2].created_by, "");
        assert_eq!(steps[2].empty_layer, false);
        assert_eq!(steps[2].created, "");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parse_full_history 2>&1 | tail -20`
Expected: FAIL — `cannot find function parse_full_history` / `cannot find type HistoryStep`.

- [ ] **Step 3: Add `HistoryStep`, `parse_full_history`, and the `ImageInfo` field**

In `src/image/mod.rs`, add the struct near the other config deserializers (after `parse_history`):

```rust
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct HistoryStep {
    pub created_by: String,
    pub empty_layer: bool,
    pub created: String,
}

pub(crate) fn parse_full_history(config: &ImageConfig) -> Vec<HistoryStep> {
    let mut steps = Vec::new();
    if let Some(history) = &config.history {
        for h in history {
            steps.push(HistoryStep {
                created_by: h.created_by.clone().unwrap_or_default(),
                empty_layer: h.empty_layer.unwrap_or(false),
                created: h.created.clone().unwrap_or_default(),
            });
        }
    }
    steps
}
```

Add the field to `ImageInfo`:

```rust
pub struct ImageInfo {
    pub layers: Vec<LayerInfo>,
    pub total_size: u64,
    pub architecture: String,
    pub os: String,
    pub source: String,
    pub history: Vec<HistoryStep>,
}
```

- [ ] **Step 4: Populate `history` at all three `ImageInfo` construction sites**

`src/image/registry.rs`:
- Add `parse_full_history` to the `use super::{...}` list at line ~11 (which already imports `parse_history`).
- After `let (commands, created_times) = parse_history(&image_config);` (line ~114) add:
  ```rust
  let history = parse_full_history(&image_config);
  ```
- Add `history,` to the `Ok(ImageInfo { ... })` literal (line ~237).

`src/image/tarball.rs`:
- Add `parse_full_history` to the `use super::{...}` at line 10.
- After `let (commands, created_times) = parse_history(&config);` (line ~103) add:
  ```rust
  let history = parse_full_history(&config);
  ```
- Add `history,` to the `Ok(ImageInfo { ... })` literal at line ~146.
- In `load_single_layer` (the bare-tar fallback with no config), add `history: Vec::new(),` to the `Ok(ImageInfo { ... })` literal at line ~178.

- [ ] **Step 5: Run tests to verify pass + build**

Run: `cargo test 2>&1 | grep "test result"` then `cargo build 2>&1 | tail -3`
Expected: all tests pass (new `parse_full_history_keeps_empty_layers_and_order` included); build clean.

- [ ] **Step 6: Commit**

```bash
git add src/image/mod.rs src/image/registry.rs src/image/tarball.rs
git commit -m "feat: parse full build history including empty-layer instructions"
```

---

### Task 2: Dockerfile reconstruction module

**Files:**
- Create: `src/dockerfile.rs`
- Modify: `src/main.rs` (add `mod dockerfile;` alongside the other `mod` lines)
- Test: `src/dockerfile.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::image::HistoryStep` (Task 1); `crate::command_format::clean_command` (existing — strips `/bin/sh -c `, `#(nop) `, collapses whitespace).
- Produces: `pub fn reconstruct(history: &[HistoryStep]) -> String`; `pub fn render_raw(history: &[HistoryStep]) -> String`.

- [ ] **Step 1: Write the failing tests**

Create `src/dockerfile.rs` with tests first:

```rust
use crate::command_format::clean_command;
use crate::image::HistoryStep;

#[cfg(test)]
mod tests {
    use super::*;

    fn step(created_by: &str, empty: bool) -> HistoryStep {
        HistoryStep {
            created_by: created_by.to_string(),
            empty_layer: empty,
            created: String::new(),
        }
    }

    #[test]
    fn reconstruct_maps_nop_env_and_plain_run() {
        let history = vec![
            step("/bin/sh -c #(nop)  ENV PATH=/usr/local/bin", true),
            step("/bin/sh -c apt-get update && apt-get install -y nginx", false),
        ];
        let out = reconstruct(&history);
        assert!(out.contains("ENV PATH=/usr/local/bin"));
        assert!(out.contains("RUN apt-get update && apt-get install -y nginx"));
        // Header present.
        assert!(out.contains("Reconstructed by imgchk"));
        assert!(out.contains("NOT a guaranteed-buildable"));
    }

    #[test]
    fn reconstruct_rewrites_legacy_copy() {
        let history = vec![step("/bin/sh -c #(nop) COPY dir:abc123def in /app", false)];
        let out = reconstruct(&history);
        assert!(out.contains("COPY <context unavailable> /app"));
        assert!(out.contains("original source not in image"));
        assert!(!out.contains("dir:abc123def"));
    }

    #[test]
    fn reconstruct_strips_buildkit_marker() {
        let history = vec![step("RUN /bin/sh -c apk add curl # buildkit", false)];
        let out = reconstruct(&history);
        assert!(out.contains("RUN apk add curl"));
        assert!(!out.contains("buildkit"));
    }

    #[test]
    fn reconstruct_passes_through_buildkit_real_copy() {
        // BuildKit can preserve a real source path — must NOT be rewritten.
        let history = vec![step("COPY ./app /app # buildkit", false)];
        let out = reconstruct(&history);
        assert!(out.contains("COPY ./app /app"));
        assert!(!out.contains("context unavailable"));
    }

    #[test]
    fn reconstruct_empty_history_notes_it() {
        let out = reconstruct(&[]);
        assert!(out.contains("No build history available"));
        assert!(out.contains("Reconstructed by imgchk"));
    }

    #[test]
    fn render_raw_is_verbatim_and_ordered() {
        let history = vec![
            step("/bin/sh -c #(nop)  ENV A=1", true),
            step("/bin/sh -c apt-get update", false),
        ];
        let out = render_raw(&history);
        assert_eq!(out, "/bin/sh -c #(nop)  ENV A=1\n/bin/sh -c apt-get update");
    }

    #[test]
    fn render_raw_empty_history() {
        assert!(render_raw(&[]).contains("No build history available"));
    }
}
```

Add `mod dockerfile;` to `src/main.rs` (alphabetically: after `mod command_format;`, before `mod extract;`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test dockerfile 2>&1 | tail -20`
Expected: FAIL — `cannot find function reconstruct` / `render_raw`.

- [ ] **Step 3: Write the implementation**

Add to `src/dockerfile.rs` above the `#[cfg(test)]` block:

```rust
const HEADER: &str = "\
# Reconstructed by imgchk from image build history.
# This is an approximation, NOT a guaranteed-buildable Dockerfile:
#   - the base image (FROM) cannot be recovered from history
#   - COPY/ADD build context is not stored in the image
# Review before use.
";

const INSTRUCTIONS: &[&str] = &[
    "RUN", "CMD", "ENV", "ENTRYPOINT", "EXPOSE", "WORKDIR", "USER", "LABEL",
    "VOLUME", "ARG", "MAINTAINER", "COPY", "ADD", "HEALTHCHECK", "STOPSIGNAL",
    "SHELL", "ONBUILD",
];

/// Render the full history as an approximate, annotated Dockerfile.
pub fn reconstruct(history: &[HistoryStep]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for step in history {
        let norm = normalize(&step.created_by);
        if norm.is_empty() {
            continue;
        }
        if let Some(rewritten) = rewrite_legacy_copy_add(&norm) {
            lines.push(rewritten);
        } else if starts_with_instruction(&norm) {
            lines.push(norm);
        } else {
            lines.push(format!("RUN {norm}"));
        }
    }
    if lines.is_empty() {
        return format!(
            "{HEADER}# No build history available in this image (squashed or history-stripped).\n"
        );
    }
    format!("{HEADER}{}\n", lines.join("\n"))
}

/// Render the verbatim ordered command list (one created_by per line).
pub fn render_raw(history: &[HistoryStep]) -> String {
    if history.is_empty() {
        return "# No build history available in this image.".to_string();
    }
    history
        .iter()
        .map(|s| s.created_by.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip `/bin/sh -c`, `#(nop)`, collapse whitespace (via clean_command), then
/// drop a trailing BuildKit ` # buildkit` marker.
fn normalize(created_by: &str) -> String {
    let cleaned = clean_command(created_by);
    cleaned
        .strip_suffix("# buildkit")
        .unwrap_or(&cleaned)
        .trim()
        .to_string()
}

fn starts_with_instruction(line: &str) -> bool {
    let first = line.split_whitespace().next().unwrap_or("");
    INSTRUCTIONS.iter().any(|k| k.eq_ignore_ascii_case(first))
}

/// Detect the legacy `COPY dir:<hash> in <dest>` / `ADD file:<hash> in <dest>`
/// form (build context not recoverable) and rewrite it to an annotated line.
/// Returns None for BuildKit lines that carry a real source path.
fn rewrite_legacy_copy_add(line: &str) -> Option<String> {
    let inst = if line.starts_with("COPY ") {
        "COPY"
    } else if line.starts_with("ADD ") {
        "ADD"
    } else {
        return None;
    };
    let rest = &line[inst.len() + 1..];
    let idx = rest.find(" in ")?;
    let src = &rest[..idx];
    let dest = rest[idx + " in ".len()..].trim();
    // Legacy source looks like "dir:<hex>" / "file:<hex>" / "multi:<hex>".
    let looks_legacy = src
        .split_once(':')
        .map(|(_, hex)| !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or(false);
    if looks_legacy {
        Some(format!(
            "{inst} <context unavailable> {dest}  # reconstructed: original source not in image"
        ))
    } else {
        None
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test dockerfile 2>&1 | tail -20`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add src/dockerfile.rs src/main.rs
git commit -m "feat: add Dockerfile reconstruction and raw-history rendering"
```

---

### Task 3: Report fields

**Files:**
- Modify: `src/report.rs` (add `history` + `dockerfile` fields, fill in `build_report`)
- Test: `src/report.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::image::{HistoryStep, ImageInfo}` (Task 1); `crate::dockerfile::reconstruct` (Task 2).
- Produces: `ReportImage.history: Vec<HistoryStep>` and `ReportImage.dockerfile: String`.

- [ ] **Step 1: Write the failing test**

Add to `src/report.rs` `#[cfg(test)] mod tests` (create the module if none exists; if one exists, add the test into it). This test builds a minimal `ImageInfo` with history and asserts the report carries it:

```rust
    #[test]
    fn build_report_includes_history_and_dockerfile() {
        use crate::image::{HistoryStep, ImageInfo};
        let image = ImageInfo {
            layers: vec![],
            total_size: 0,
            architecture: "amd64".into(),
            os: "linux".into(),
            source: "test:latest".into(),
            history: vec![
                HistoryStep {
                    created_by: "/bin/sh -c #(nop)  ENV A=1".into(),
                    empty_layer: true,
                    created: "t0".into(),
                },
                HistoryStep {
                    created_by: "/bin/sh -c apt-get update".into(),
                    empty_layer: false,
                    created: "t1".into(),
                },
            ],
        };
        let report = build_report(&image);
        assert_eq!(report.history.len(), 2);
        assert_eq!(report.history[0].created_by, "/bin/sh -c #(nop)  ENV A=1");
        assert!(report.dockerfile.contains("ENV A=1"));
        assert!(report.dockerfile.contains("RUN apt-get update"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test build_report_includes_history 2>&1 | tail -20`
Expected: FAIL — no field `history`/`dockerfile` on `ReportImage`.

- [ ] **Step 3: Add the fields and fill them**

In `src/report.rs`, add to the `ReportImage` struct (after the existing fields, before `layers` is fine; order only affects JSON key order):

```rust
    pub history: Vec<crate::image::HistoryStep>,
    pub dockerfile: String,
```

In `build_report`, add to the returned `ReportImage { ... }`:

```rust
        history: image.history.clone(),
        dockerfile: crate::dockerfile::reconstruct(&image.history),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test report 2>&1 | tail -20`
Expected: PASS (new test + existing report tests).

- [ ] **Step 5: Commit**

```bash
git add src/report.rs
git commit -m "feat: add history and reconstructed dockerfile to --report JSON"
```

---

### Task 4: CLI flag + standalone output

**Files:**
- Modify: `src/main.rs` (`DockerfileMode` enum, `dockerfile` field, validation, standalone branch)
- Test: `src/main.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `dockerfile::reconstruct`, `dockerfile::render_raw` (Task 2); existing `scan`/`report` fields on `Cli`.
- Produces: `DockerfileMode { Reconstructed, Raw }`; `Cli.dockerfile: Option<DockerfileMode>`; `fn validate_dockerfile_args(cli: &Cli) -> anyhow::Result<()>`.

- [ ] **Step 1: Write the failing tests**

Add to `src/main.rs` `mod tests`:

```rust
    #[test]
    fn cli_dockerfile_bare_defaults_to_reconstructed() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest", "--dockerfile"]);
        assert_eq!(cli.dockerfile, Some(DockerfileMode::Reconstructed));
    }

    #[test]
    fn cli_dockerfile_raw_parses() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest", "--dockerfile=raw"]);
        assert_eq!(cli.dockerfile, Some(DockerfileMode::Raw));
    }

    #[test]
    fn validate_dockerfile_args_rejects_dockerfile_plus_scan_without_report() {
        let cli = Cli::parse_from([
            "imgchk",
            "nginx:latest",
            "--dockerfile",
            "--scan",
            "trivy",
        ]);
        assert!(validate_dockerfile_args(&cli).is_err());
    }

    #[test]
    fn validate_dockerfile_args_allows_dockerfile_plus_scan_with_report() {
        let cli = Cli::parse_from([
            "imgchk",
            "nginx:latest",
            "--report",
            "--dockerfile",
            "--scan",
            "trivy",
        ]);
        assert!(validate_dockerfile_args(&cli).is_ok());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test dockerfile 2>&1 | tail -20`
Expected: FAIL — `cannot find type DockerfileMode` / field `dockerfile` / fn `validate_dockerfile_args`.

- [ ] **Step 3: Add the enum, flag, and validation**

In `src/main.rs`, define the enum near `Cli` (after the imports / before or after `Cli`):

```rust
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum DockerfileMode {
    Reconstructed,
    Raw,
}
```

Add the field to the `Cli` struct (after the `scan_cmd` field):

```rust
    /// Print the image's build instructions instead of launching the TUI.
    /// Bare --dockerfile prints an approximate (best-effort, not guaranteed
    /// buildable) Dockerfile; --dockerfile=raw prints the verbatim ordered
    /// command list. Ignored for output selection when --report is set
    /// (--report always includes history + dockerfile JSON fields).
    #[arg(
        long,
        value_enum,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "reconstructed"
    )]
    dockerfile: Option<DockerfileMode>,
```

Add the validation function near `validate_scan_args`:

```rust
/// `--dockerfile` and `--scan` both want to own stdout in their standalone
/// (non-`--report`) human modes; combining them without `--report` would
/// silently ignore one, so reject it.
fn validate_dockerfile_args(cli: &Cli) -> anyhow::Result<()> {
    if cli.dockerfile.is_some() && cli.scan.is_some() && !cli.report {
        anyhow::bail!("--dockerfile and --scan cannot be combined without --report");
    }
    Ok(())
}
```

Call it in `main()` right after `validate_scan_args(&cli)?;`:

```rust
    validate_dockerfile_args(&cli)?;
```

- [ ] **Step 4: Add the standalone print branch in `main()`**

In `main()`, after the image is loaded and the empty-layers check, and BEFORE the `if let Some(tool) = cli.scan { ... }` scan branch, add:

```rust
    if let Some(mode) = cli.dockerfile {
        if !cli.report {
            let text = match mode {
                DockerfileMode::Reconstructed => dockerfile::reconstruct(&image.history),
                DockerfileMode::Raw => dockerfile::render_raw(&image.history),
            };
            println!("{text}");
            return Ok(());
        }
        // With --report, fall through: the report branch already emits
        // history + dockerfile.
    }
```

(The existing `if cli.report { ... }` branch already calls `build_report`, which now includes the fields — no change needed there beyond Task 3.)

- [ ] **Step 5: Run the full suite**

Run: `cargo test 2>&1 | grep "test result"`
Expected: PASS (all new CLI tests + everything else).

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: add --dockerfile flag with reconstructed/raw modes"
```

---

### Task 5: Documentation + verification

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `src/main.rs` (`EXAMPLES_HELP`)

**Interfaces:**
- Consumes: the finished feature. Produces: docs only.

- [ ] **Step 1: Verify standalone reconstructed output end-to-end**

Run:
```bash
cargo run -- alpine:3.19 --dockerfile
```
Expected: the header comment block, then reconstructed instructions (e.g. `ENV`, `RUN`, `CMD`) from alpine's history. Exits 0. (Pulls alpine:3.19 over the network; if unavailable, note it and rely on unit tests.)

- [ ] **Step 2: Verify raw mode and report fields**

Run:
```bash
cargo run -- alpine:3.19 --dockerfile=raw
cargo run -- alpine:3.19 --report | jq '{history_len: (.history | length), has_dockerfile: (.dockerfile != null), first: .history[0]}'
```
Expected: raw prints verbatim `created_by` lines; the report shows a non-empty `history` array and a `dockerfile` string.

- [ ] **Step 3: Verify the flag clash is rejected**

Run:
```bash
cargo run -- alpine:3.19 --dockerfile --scan trivy; echo "exit=$?"
```
Expected: prints the `--dockerfile and --scan cannot be combined without --report` error and a non-zero exit.

- [ ] **Step 4: Update `README.md`**

- Add a `## Features` bullet for `--dockerfile` (reconstructed Dockerfile / raw command list).
- Add a "Build history & Dockerfile reconstruction" subsection: the two modes (`--dockerfile`, `--dockerfile=raw`), the `--report` `history` (array of `{created_by, empty_layer, created}`) and `dockerfile` (string) fields, and an explicit **limitations** note — best-effort, not guaranteed-buildable; no `FROM` (base boundary unrecoverable); `COPY`/`ADD` context lost; BuildKit vs legacy differences; squashed images carry little history.
- Add a jq recipe, e.g.:
  ```bash
  imgchk nginx:latest --report | jq -r '.dockerfile'
  ```

- [ ] **Step 5: Update `CLAUDE.md`**

Add an item under `## Core Behavior` describing `--dockerfile`: two modes (reconstructed / `=raw`), history-only reconstruction that INCLUDES empty-layer instructions (`ENV`/`CMD`/etc.), the `--report` `history`+`dockerfile` fields, and the not-guaranteed-buildable limitation. Implemented in `src/dockerfile.rs`.

- [ ] **Step 6: Update `--help` (`EXAMPLES_HELP` in `src/main.rs`)**

Add an example:

```
    Reconstruct an approximate Dockerfile from the image's build history:
        imgchk nginx:latest --dockerfile
```

- [ ] **Step 7: Build, confirm help, commit**

```bash
cargo build --release 2>&1 | tail -3 && cargo run -- --help | grep -A1 "Reconstruct an approximate"
git add README.md CLAUDE.md src/main.rs
git commit -m "docs: document --dockerfile reconstruction and --report history/dockerfile fields"
```

---

## Self-Review

**Spec coverage:**
- `--dockerfile` value-enum, bare→reconstructed, `=raw`→raw → Task 4. ✓
- Full history incl. empty-layer entries (`parse_full_history`, `ImageInfo.history`), existing `parse_history` untouched → Task 1. ✓
- Reconstruction rules (header, keyword detection, `RUN` fallback, legacy COPY/ADD rewrite, BuildKit marker strip, empty history) → Task 2. ✓
- `render_raw` verbatim/ordered → Task 2. ✓
- `history` + `dockerfile` always in `--report` with exact field names → Task 3. ✓
- History-only (no config) → Tasks 2/3 use only `image.history`. ✓
- No `FROM`, COPY context annotated, not-buildable note → Tasks 2 & 5. ✓
- `--dockerfile` + `--scan` without `--report` rejected → Task 4. ✓
- Standalone print branch placed before scan branch; report fall-through → Task 4. ✓
- Docs (README/CLAUDE.md/`--help`) + limitations → Task 5. ✓

**Placeholder scan:** No TBD/TODO; every code step has complete code.

**Type consistency:** `HistoryStep{created_by,empty_layer,created}` (Task 1) used identically in Tasks 2/3; `reconstruct`/`render_raw(&[HistoryStep]) -> String` (Task 2) consumed in Tasks 3/4; `DockerfileMode{Reconstructed,Raw}` + `Cli.dockerfile: Option<DockerfileMode>` + `validate_dockerfile_args` (Task 4) match their tests. `image.history` field name consistent across image/report/main. ✓
