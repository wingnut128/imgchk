# imgchk

Container image inspector and layer extraction tool.

## Project Overview

imgchk fetches OCI/Docker container images, displays layer metadata in an interactive TUI, and extracts layers to disk in multiple formats.

## Architecture

- **Language:** Rust (2021 edition)
- **Key dependencies:**
  - `ocirender` (Edera) — OCI image rendering to squashfs, tar, and directory formats. Supports streaming conversion with parallel layer downloads.
  - `ratatui` — TUI framework
  - `oci-distribution` or `oci-client` — registry pulls
  - `tokio` — async runtime for parallel layer fetching
  - `clap` — CLI argument parsing

## Core Behavior

1. **Image fetching:** Pull image manifest and layers in parallel from registries, Docker daemon, or local tarballs
2. **TUI:** Interactive terminal UI with three panes:
   - **Layers** (left) — list of layers with index, size, and command. Press `e` here to export all layers as tar.gz files
   - **Files** (right top) — file tree for the selected layer. Toggle cumulative view with `t`. Select files with `space`, extract with `e`
   - **Details** (right bottom) — metadata for the selected layer (command, digest, diffID, size, created, file count)
3. **Extraction:** Use `ocirender` for output. Supported formats: squashfs, tar, directory extraction
4. **Output directory:** Set via `-o` flag or `o` keybinding in the TUI. If unset, creates a tmpdir on first extraction and displays the full path in the status bar
5. **Report mode:** `--report` skips the TUI entirely and prints a JSON analysis (per-layer metadata + suspicious-file findings: setuid/setgid/world-writable/secret-pattern) to stdout, for CI/scripting use. Implemented in `src/report.rs`. No exit-code gating and no signature verification in this mode — `signature` is reserved (always `null`) for a future spec.
6. **Vulnerability scanning:** `--scan <trivy|grype|custom>` (requires `--report`) extracts the merged filesystem to a tempdir and shells out to an external scanner, embedding its raw JSON (or raw stdout) output under a top-level `scan` field. Implemented in `src/scan.rs`. `--scan=custom` requires `--scan-cmd '<template>'` with a `{path}` placeholder. A scan failure surfaces in `scan.error` without blocking the rest of the report. No normalization of scanner output, no exit-code gating, no timeout, no TUI integration.

## TUI Keybindings

| Key     | Action                          |
|---------|---------------------------------|
| `j`/`k` | Navigate up/down                |
| `tab`   | Cycle pane focus                |
| `enter` | Expand/collapse directory       |
| `space` | Select/deselect file or dir     |
| `t`     | Toggle layer/cumulative view    |
| `o`     | Set output directory            |
| `e`     | Extract (files or layers)       |
| `q`     | Quit                            |

## Build & Run

Common tasks are wrapped in a `justfile` ([just](https://github.com/casey/just) is the task runner). Run `just` or `just --list` to see all recipes.

```sh
just build          # debug build
just release        # optimized release build
just run <image-ref> # build + run (e.g. just run nginx.tar)
just hooks          # install the git pre-commit hook (fmt + clippy)
```

Or invoke cargo directly:

```sh
cargo build --release
cargo run -- <image-ref>
# Examples:
#   imgchk nginx:latest
#   imgchk ghcr.io/org/app:v1.2 --platform linux/arm64
#   imgchk -o /tmp/extracted alpine:3.19
```

## Testing

```sh
just test    # or: cargo test
```

## Design Decisions

- Layers are fetched in parallel; `ocirender`'s `StreamingPacker` accepts layers in any arrival order and resequences them
- File tree is built by reading tar headers from each layer, not by extracting to disk
- Whiteout semantics (`.wh.*` files, opaque dirs) are handled during tree merge for cumulative view
- The TUI is the primary interface; all extraction feedback (counts, paths) is shown in the status bar

## Agent skills

### Issue tracker

Issues are tracked in Linear (team `ENG`) via the Linear MCP. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical defaults — no remapping. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — `CONTEXT.md` and `docs/adr/` at repo root. See `docs/agents/domain.md`.
