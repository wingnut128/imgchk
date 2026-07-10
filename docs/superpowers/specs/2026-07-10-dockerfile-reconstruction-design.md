# `--dockerfile`: extract & reconstruct build instructions

## Problem

imgchk already parses each image's build history (`history[].created_by` from
the OCI image config) into per-layer `command` strings shown in the TUI and
`--report`. But two things are missing:

1. **Empty-layer instructions are dropped.** `parse_history`
   (`src/image/mod.rs:85`) skips history entries with `empty_layer == true`,
   because those don't map to a filesystem blob. That discards real Dockerfile
   instructions — `ENV`, `WORKDIR`, `CMD`, `ENTRYPOINT`, `EXPOSE`, `USER`,
   `LABEL`, `VOLUME`, `ARG` — which produce no layer but are part of the build.
2. **There's no way to see the build as a whole.** A user auditing an image
   has to eyeball per-layer commands in the TUI; there's no ordered command
   list and no Dockerfile-shaped view.

## Goal

Add a `--dockerfile` flag that surfaces an image's build steps two ways:

- **Reconstructed Dockerfile** (default): a best-effort, annotated Dockerfile
  rendered from the full ordered history.
- **Raw command list** (`--dockerfile=raw`): the verbatim ordered
  `created_by` strings, including empty-layer instructions.

In `--report`, the same data is exposed as JSON: an ordered `history` array and
a reconstructed `dockerfile` string.

## Honesty / non-goals

The reconstructed Dockerfile is an **approximation for understanding and
auditing — not a guaranteed-buildable file**. This is inherent to the data, not
a v1 shortcut:

- `COPY`/`ADD` history records `dir:<hash> in <dest>` (legacy) — the original
  build context/source is not in the image, so those lines cannot be made
  buildable. (BuildKit sometimes preserves a real source path; when it does, we
  keep it.)
- The **base-image boundary is invisible**: history includes the base image's
  own layers inline, with no marker for where `FROM <base>` ended and the
  user's Dockerfile began. We therefore emit **no `FROM` line**, only an
  annotated comment.
- BuildKit and legacy builders format `created_by` differently; squashed or
  history-stripped images carry little or no history.
- **Multi-stage builds**: only the final stage's history survives in the image.

Also explicitly out of scope for this iteration:

- No pulling final directives from the image *config* (`Config.Cmd`,
  `Config.Env`, ...). Reconstruction is **history-only** — history is the build
  record; mixing in config risks duplicated/conflicting lines.
- No pretty-printing of `RUN` commands into multi-line `\`-continuations —
  `RUN` lines are emitted verbatim (after prefix stripping) for fidelity.
- No TUI integration.
- No exit-code gating.

## CLI surface

New flag on `Cli` (`src/main.rs`):

```
--dockerfile [<mode>]   Print the image's build instructions instead of
                        launching the TUI. Mode is one of:
                          reconstructed  (default) an approximate Dockerfile
                          raw            the verbatim ordered command list
```

- Implemented as a `clap` value-enum with an optional value: bare
  `--dockerfile` selects `reconstructed`; `--dockerfile=raw` selects `raw`.
  (clap: `value_enum`, `num_args = 0..=1`, `require_equals = true`,
  `default_missing_value = "reconstructed"`.) `require_equals = true` is
  required so the value form is `--dockerfile=raw`, not `--dockerfile raw` —
  the space form would ambiguously consume the positional image reference.
- Standalone (no `--report`): prints the selected text to stdout, no TUI.
- With `--report`: the `--dockerfile` value is ignored for output selection —
  `--report` always emits both `history` and `dockerfile` JSON fields (see
  below). `--dockerfile` combined with `--report` is allowed and simply
  redundant (no error).
- No interaction with `--scan`/`--scan-cmd`; independent flag.

## Data layer

New in `src/image/mod.rs`:

```rust
#[derive(Clone, Debug)]
pub(crate) struct HistoryStep {
    pub created_by: String,   // verbatim history created_by
    pub empty_layer: bool,    // true = no filesystem layer (ENV/CMD/...)
    pub created: String,      // timestamp, "" if absent
}

pub(crate) fn parse_full_history(config: &ImageConfig) -> Vec<HistoryStep>;
```

- Preserves order and **includes** empty-layer entries (unlike `parse_history`,
  which is left untouched so existing layer↔blob alignment keeps working).
- Missing `created_by`/`created` → empty strings; missing `empty_layer` →
  `false`.

Add `pub history: Vec<HistoryStep>` to `ImageInfo` (`src/image/mod.rs`),
populated in both `src/image/registry.rs` and `src/image/tarball.rs` right
where `parse_history` is already called (`registry.rs:114`, `tarball.rs:103`).
For the tarball path with no history, this is an empty vec.

## Reconstruction module

New `src/dockerfile.rs` — pure functions, no I/O:

```rust
use crate::image::HistoryStep;

/// Render the full history as an approximate, annotated Dockerfile.
pub fn reconstruct(history: &[HistoryStep]) -> String;

/// Render the verbatim ordered command list (one created_by per line).
pub fn render_raw(history: &[HistoryStep]) -> String;
```

### `reconstruct` rules

Header (always): a comment block —
```
# Reconstructed by imgchk from image build history.
# This is an approximation, NOT a guaranteed-buildable Dockerfile:
#   - the base image (FROM) cannot be recovered from history
#   - COPY/ADD build context is not stored in the image
# Review before use.
```

Per step, in order:
1. Normalize `created_by`: strip a leading `/bin/sh -c ` and any `#(nop) `
   marker; strip a trailing ` # buildkit`; collapse internal whitespace runs to
   single spaces (reuse `command_format::clean_command`, which already strips
   `/bin/sh -c ` and `#(nop) ` and collapses whitespace).
2. If the normalized text is empty, skip it.
3. If it starts (case-insensitive) with a known instruction keyword — `ENV`,
   `CMD`, `ENTRYPOINT`, `EXPOSE`, `WORKDIR`, `USER`, `LABEL`, `VOLUME`, `ARG`,
   `MAINTAINER`, `COPY`, `ADD`, `RUN`, `HEALTHCHECK`, `STOPSIGNAL`,
   `SHELL` — emit it as-is (it already reads as that instruction).
4. Else emit `RUN <normalized>`.
5. **COPY/ADD special case**: if the line matches the legacy pattern
   `COPY dir:<hash> in <dest>` or `ADD file:<hash> in <dest>` (i.e. contains a
   `<something>:<hex> in ` segment), rewrite to
   `COPY <context unavailable> <dest>  # reconstructed: original source not in image`
   (same for `ADD`). BuildKit lines with a real path fall through step 3
   unchanged.

If `history` is empty (or every step normalizes away), return the header plus a
single line:
`# No build history available in this image (squashed or history-stripped).`

### `render_raw` rules

- One `created_by` per line, verbatim, in original order (empty-layer entries
  included).
- Empty history → the single line
  `# No build history available in this image.`

## Report shape

Two new top-level fields on `ReportImage` (`src/report.rs`), always present:

```json
{
  "source": "nginx:latest",
  "...": "... existing fields ...",
  "history": [
    { "created_by": "/bin/sh -c #(nop)  ENV PATH=/usr/local/bin", "empty_layer": true,  "created": "2026-01-01T00:00:00Z" },
    { "created_by": "/bin/sh -c apt-get update && apt-get install -y nginx", "empty_layer": false, "created": "2026-01-01T00:00:01Z" }
  ],
  "dockerfile": "# Reconstructed by imgchk ...\nENV PATH=/usr/local/bin\nRUN apt-get update && apt-get install -y nginx\n"
}
```

- `history`: the full ordered `parse_full_history` output (serialized
  `{created_by, empty_layer, created}`).
- `dockerfile`: the `reconstruct(&history)` string.
- Both are additive keys (non-breaking for existing `jq` consumers). A
  serializable mirror of `HistoryStep` lives in `report.rs` (or `HistoryStep`
  gains `Serialize`) — implementer's choice, but the JSON field names must be
  exactly `created_by`, `empty_layer`, `created`.

## Execution flow (`src/main.rs`)

After `Cli::parse()` / existing validation and after the image is loaded:

```rust
if let Some(mode) = cli.dockerfile {
    let text = match mode {
        DockerfileMode::Reconstructed => dockerfile::reconstruct(&image.history),
        DockerfileMode::Raw => dockerfile::render_raw(&image.history),
    };
    if cli.report {
        // fall through to the report branch below (which now always includes
        // history + dockerfile); do not also print the text.
    } else {
        println!("{text}");
        return Ok(());
    }
}

if cli.report {
    let report = report::build_report(&image);   // build_report now fills history + dockerfile
    // ... existing scan attach ...
    println!("{}", serde_json::to_string_pretty(&report)?);
    return Ok(());
}
```

`build_report` gains the `history` and `dockerfile` fields (computed from
`image.history` via `dockerfile::reconstruct`). To keep `build_report` a pure
transform, `reconstruct`/`render_raw` are pure and take `&[HistoryStep]`.

Note the scan branch (`if let Some(tool) = cli.scan`) already returns early; the
`--dockerfile` standalone branch is placed alongside it. `--dockerfile` and
`--scan` standalone together is not a meaningful combination — if both are
given without `--report`, `--scan` takes precedence (document it); or reject the
combination in validation. **Decision: reject `--dockerfile` + `--scan` without
`--report`** with a clear error, mirroring the existing `validate_scan_args`
style, to avoid silently ignoring one flag.

## Implementation shape

- **`src/image/mod.rs`**: add `HistoryStep`, `parse_full_history`, and
  `ImageInfo.history`.
- **`src/image/registry.rs`, `src/image/tarball.rs`**: populate `history`.
- **`src/dockerfile.rs`** (new): `reconstruct`, `render_raw`, plus private
  helpers (keyword detection, COPY/ADD legacy rewrite). Reuses
  `command_format::clean_command`.
- **`src/main.rs`**: `DockerfileMode` value-enum, `dockerfile: Option<...>`
  field, validation for the `--dockerfile` + `--scan` standalone clash, the
  standalone print branch.
- **`src/report.rs`**: `history` + `dockerfile` fields on `ReportImage`,
  filled in `build_report`.

## Testing

- `parse_full_history`: keeps empty-layer entries and order; maps missing
  fields to defaults. (Unit test in `src/image/mod.rs`.)
- `reconstruct` (`src/dockerfile.rs`):
  - legacy `#(nop) ENV`, `#(nop) CMD`, `#(nop) WORKDIR`, `#(nop) EXPOSE` →
    correct bare instruction
  - legacy `COPY dir:<hash> in /app` → `COPY <context unavailable> /app  # ...`
  - BuildKit `RUN /bin/sh -c ... # buildkit` → `RUN ...` (buildkit marker
    stripped)
  - plain shell command (no keyword) → `RUN <cmd>`
  - already-`RUN`/`ENV`-prefixed BuildKit line → passed through unchanged
  - empty history → header + "no build history" comment
  - header comment block present
- `render_raw`: verbatim lines in order incl. empty-layer entries; empty
  history → single comment.
- `main.rs`: `--dockerfile` parses (bare → reconstructed); `--dockerfile=raw`
  parses; `--dockerfile` + `--scan` without `--report` errors.

## Documentation

Per the project's standing rule, updated in the same change:

- `README.md`: feature bullet; a "Build history / Dockerfile" subsection
  documenting the two modes, the `--report` `history`/`dockerfile` fields, and
  the **explicit limitation note** (not guaranteed buildable; no FROM; COPY
  context lost); a jq recipe reading `.history` or `.dockerfile`.
- `CLAUDE.md`: a new item under `## Core Behavior` describing `--dockerfile`
  (two modes, history-only reconstruction, empty-layer instructions included,
  limitations), implemented in `src/dockerfile.rs`.
- `--help` (`EXAMPLES_HELP` in `src/main.rs`): a `--dockerfile` usage example.
