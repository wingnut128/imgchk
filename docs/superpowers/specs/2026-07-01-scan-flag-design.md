# `--scan` external vulnerability scanning

Linear: ENG-114

## Problem

imgchk's `--report` mode (see `2026-06-30-report-mode-design.md`) does heuristic
file-permission and filename-pattern checks (`suspicious_files`) — setuid,
setgid, world-writable, secret-pattern names. That's useful signal, but it's
not real vulnerability/CVE scanning, and it isn't trying to be: matching
installed package versions against a CVE database is a different, much
larger problem that mature free/open-source tools already solve well (Trivy,
Grype). Reimplementing a vuln-database pipeline inside imgchk would duplicate
those tools poorly.

## Goal

Add a `--scan` flag that shells out to an external scanner against the
image's fully-merged filesystem and embeds the scanner's own output in the
`--report` JSON. The mechanism is generic — imgchk ships convenience presets
for Trivy and Grype, but any scanner works via a custom command template.
imgchk does not parse, normalize, or interpret scanner-specific findings; it
just plumbs the command through and captures the result.

## Non-goals (v1)

- No timeout on the external scanner command — it runs to completion or the
  user interrupts imgchk.
- No installing, bundling, or version-checking scanner binaries — imgchk
  assumes whatever the user names is already on `PATH` (or is a shell command
  that resolves on its own, e.g. `docker run ...`).
- No TUI integration — `--scan` requires `--report` and has no interactive
  pane or keybinding.
- No normalization of scanner output into imgchk's own `suspicious_files`
  shape — Trivy/Grype output stays in its own tool-specific JSON shape,
  embedded verbatim under `scan.output`.
- No exit-code gating on scan findings — consistent with `--report`'s
  existing philosophy; pipe `scan.output` through `jq` and gate in CI as
  needed.

## CLI surface

New flags on the existing `Cli` struct in `src/main.rs`:

```
--scan <trivy|grype|custom>   Run an external scanner against the merged image
                               filesystem and embed its output in the report.
                               Requires --report.
--scan-cmd <template>          Custom scanner command template. Required when
                               --scan=custom; rejected otherwise. Use {path}
                               as a placeholder for the extracted rootfs
                               directory.
```

Validation (checked in `main()` immediately after CLI parsing, before any
image fetch):
- `--scan` without `--report` → error, exit non-zero, no image fetch.
- `--scan=custom` without `--scan-cmd` → error.
- `--scan-cmd` given with `--scan=trivy` or `--scan=grype` → error (avoids
  silently ignoring a flag the user set).

Built-in preset command templates:

| `--scan` value | Expands to |
|----------------|------------|
| `trivy`        | `trivy rootfs --format json {path}` |
| `grype`        | `grype dir:{path} -o json` |
| `custom`       | whatever `--scan-cmd` specifies |

## Execution flow

1. `main()` resolves the image and builds the JSON report exactly as
   `--report` does today (unchanged).
2. If `--scan` is set:
   a. Create a fresh temporary directory (`tempfile::tempdir()` — `tempfile`
      is already a regular dependency, used elsewhere in the codebase).
   b. Call the existing `extract::export_ocirender(&image.layers,
      ImageSpec::Dir { path: tempdir_path })` to materialize the merged,
      whiteout-resolved filesystem — the same merge logic the TUI's "export
      all layers" (`a` key) path already uses. This tempdir is independent of
      `-o` / `cli.output` — always ephemeral, always cleaned up after the
      scan regardless of what `-o` was set to.
   c. Resolve the command template: substitute every occurrence of the
      literal string `{path}` with the tempdir's path.
   d. Run the resolved command via `sh -c "<resolved command>"`. imgchk has
      no existing Windows-specific support anywhere in the codebase (the
      squashfs export format and its install instructions are Linux/macOS
      only) — `--scan` is Unix-only (`sh -c`) for the same reason; no
      Windows branch needed. This is the user's own command, running as the
      user's own process — no privilege boundary is crossed, so shelling
      out this way is not a new injection risk beyond what running any
      command the user typed themselves would be.
   e. Capture stdout, stderr, and exit code. Tempdir is removed after the
      command returns (success or failure) — use a scope guard or explicit
      cleanup so a panicking command doesn't leak the directory.
3. Print the JSON report (unchanged serialization path via
   `serde_json::to_string_pretty`).

## Report shape

New top-level field on `ReportImage` (see `src/report.rs`), `null` when
`--scan` isn't passed:

```json
{
  "source": "nginx:latest",
  "...": "... (existing fields unchanged) ...",
  "scan": {
    "tool": "trivy",
    "command": "trivy rootfs --format json /tmp/imgchk-scan-xyz",
    "exit_code": 0,
    "output": { "...": "trivy's own JSON output, embedded as-is" },
    "error": null
  }
}
```

Field semantics:
- `tool`: the value passed to `--scan` (`"trivy"`, `"grype"`, or `"custom"`).
- `command`: the fully-resolved command string actually executed (after
  `{path}` substitution) — useful for debugging/reproducing.
- `exit_code`: the scanner process's exit code, or `null` if the command
  could not be spawned at all (e.g. binary not found).
- `output`: if stdout parses as valid JSON, embedded as a JSON value;
  otherwise embedded as a JSON string containing the raw stdout. `null` if
  the command couldn't be spawned.
- `error`: `null` on a normal run (regardless of the scanner's own exit code
  — a non-zero exit from Trivy/Grype because they *found* vulnerabilities is
  not an imgchk-level error). Set to a short message only when imgchk itself
  couldn't execute the command (e.g. `"command not found: trivy"`,
  `"failed to spawn: <os error>"`). A scan command failure never blocks or
  changes the rest of the report — layers and suspicious_files always print.

## Implementation shape

- New module `src/scan.rs`:
  - `pub enum ScanTool { Trivy, Grype, Custom }` (maps to `--scan`'s clap
    value, likely via `clap::ValueEnum`).
  - `pub struct ScanResult` (Serialize) matching the shape above.
  - `pub fn resolve_command(tool: ScanTool, custom_cmd: Option<&str>, path:
    &Path) -> String` — pure function, builds the final command string from
    the preset table or the custom template. Unit-testable without spawning
    anything.
  - `pub fn run_scan(tool: ScanTool, custom_cmd: Option<&str>, layers:
    &[LayerInfo]) -> ScanResult` — does the tempdir/export/spawn/capture
    work. Not easily unit-testable (spawns real processes); covered by a
    thin integration-style test using a trivial always-available command
    (e.g. `echo`) as a stand-in `custom` scanner, not real Trivy/Grype.
- `src/main.rs` changes: add `scan: Option<ScanTool>` and `scan_cmd:
  Option<String>` to `Cli`; validation block right after `Cli::parse()`;
  call `scan::run_scan(...)` and attach to the report struct before
  serializing, inside the existing `if cli.report { ... }` branch.
- `src/report.rs` changes: add `pub scan: Option<ScanResult>` field to
  `ReportImage`. `build_report`'s signature is unchanged (still `fn
  build_report(image: &ImageInfo) -> ReportImage`, always setting `scan:
  None`) — `main.rs` sets `report.scan = Some(scan::run_scan(...))` after
  calling `build_report`, only when `cli.scan` is set. Keeps `build_report`
  a pure transform of `ImageInfo` with no knowledge of scanning, and keeps
  the scan side-effect (spawning a process) entirely in `main.rs`/`scan.rs`.

## Testing

- `resolve_command` unit tests: each preset expands correctly with a sample
  path; custom template substitutes `{path}`, including a template with
  multiple `{path}` occurrences.
- CLI validation unit tests (alongside existing `cli_*` tests in
  `main.rs`): `--scan` without `--report` errors; `--scan=custom` without
  `--scan-cmd` errors; `--scan-cmd` with `--scan=trivy` errors; valid
  combinations parse successfully.
- `run_scan` integration test using a fake "scanner" (e.g. `--scan=custom
  --scan-cmd='echo {\"ok\":true}'`) to verify: tempdir is created and passed
  correctly, stdout is captured and parsed as JSON, tempdir is cleaned up
  after the call returns Exercise both a valid-JSON-stdout case and a
  non-JSON-stdout case (raw string fallback), and a command-not-found case
  (`error` populated, `output`/`exit_code` null).
