# Friendlier `--scan` output: normalized summary + human rendering

## Problem

`--scan` (see `2026-07-01-scan-flag-design.md`) embeds a scanner's entire raw
JSON verbatim under `scan.output`. For a real image Trivy emits an enormous
tree — every installed package, every CVE, full descriptions, references, and
layer metadata. Two consequences:

1. There is no human-facing mode at all. `--scan` requires `--report`, so the
   only output is a giant JSON blob. A person eyeballing an image has to pipe
   through `jq` and already know Trivy's schema to get "how many criticals?".
2. Even for scripting, consumers must learn each scanner's tool-specific shape
   (Trivy and Grype differ) to extract the same basic facts.

## Goal

Give `--scan` two friendlier faces over the same run:

- A **human-readable summary** printed to the terminal when `--scan` is used
  without `--report` — severity counts plus a compact table of the findings
  that matter most.
- A **normalized `scan.summary`** embedded in the `--report` JSON — a small,
  tool-agnostic shape (counts + a flat vulnerability list) that a consumer can
  read without knowing whether Trivy or Grype produced it.

Normalization is best-effort and covers the two built-in presets (Trivy,
Grype) only. Custom scanners and any output that doesn't parse fall back
gracefully: no summary, raw output retained.

## Non-goals (this iteration)

- No normalization of `custom` scanner output — their schemas are arbitrary.
  `custom` (and any parse failure) yields `summary: null`; the raw
  `scan.output` is still present.
- No heuristic/guessing parser over unknown JSON — only the known Trivy and
  Grype schemas are parsed. Guessing risks reporting wrong counts, which is
  worse than no summary.
- No new severity-threshold flag. The terminal table shows Critical + High;
  everything else is a footer count pointing to `--report`. (Revisit only if
  users ask.)
- No exit-code gating in either mode — consistent with `--report`'s existing
  philosophy. A scanner that exits non-zero because it *found* vulnerabilities
  is still a normal run; imgchk exits 0.
- No change to how the scanner is executed (tempdir extraction, `sh -c`,
  failure handling) — that flow from the prior spec is unchanged.
- No TUI integration.

## CLI surface

One rule removed, no new flags:

- **Drop** `--scan`'s `requires = "report"` constraint. `--scan` is now valid
  standalone.
- `--scan-cmd` still requires `--scan=custom`, and `--scan=custom` still
  requires `--scan-cmd` (validation unchanged).

Resulting modes:

| Invocation | Output |
|------------|--------|
| `imgchk img --scan trivy` | Human-readable summary to stdout. No TUI, no JSON. |
| `imgchk img --scan trivy --report` | JSON report with `scan.summary` embedded (raw `scan.output` retained). |
| `imgchk img` (no `--scan`) | TUI, exactly as today. |
| `imgchk img --report` (no `--scan`) | JSON report, exactly as today. |

`--scan custom --scan-cmd '...'` behaves the same in both modes; standalone it
prints a "no summary available for custom scanners — showing raw output"
notice followed by the raw stdout, since imgchk can't normalize it.

## Normalized summary shape

New module `src/scan_summary.rs`:

```rust
pub enum Severity { Critical, High, Medium, Low, Unknown }   // Ord: Critical is highest

pub struct Vulnerability {
    pub id: String,                     // e.g. "CVE-2025-1234"
    pub package: String,                // affected package name
    pub installed_version: String,
    pub fixed_version: Option<String>,  // None when no fix is available
    pub severity: Severity,
}

pub struct SeverityCounts {             // Serialize as an object of usize
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub unknown: usize,
}

pub struct ScanSummary {
    pub counts: SeverityCounts,
    pub total: usize,                   // == sum of counts
    pub vulnerabilities: Vec<Vulnerability>,   // all severities, sorted desc by severity then id
}
```

Serialized inside the report:

```json
"scan": {
  "tool": "trivy",
  "command": "trivy rootfs --format json /tmp/.tmpXXXX",
  "exit_code": 0,
  "summary": {
    "counts": { "critical": 3, "high": 12, "medium": 40, "low": 8, "unknown": 1 },
    "total": 64,
    "vulnerabilities": [
      { "id": "CVE-2025-1234", "package": "openssl", "installed_version": "3.0.11", "fixed_version": "3.0.14", "severity": "critical" }
    ]
  },
  "output": { "...": "raw scanner JSON, unchanged" },
  "error": null
}
```

- `summary` is `null` for `custom`, for unparseable output, and when the scan
  failed to run (`error` set).
- `output` (raw) is **retained** in every case — additive, non-breaking for
  existing `jq '.scan.output'` consumers.

### Parsing

```rust
pub fn summarize(tool: ScanTool, raw: &serde_json::Value) -> Option<ScanSummary>;
```

Dispatches by tool:
- `Trivy` → parse `.Results[].Vulnerabilities[]`, reading
  `VulnerabilityID`, `PkgName`, `InstalledVersion`, `FixedVersion`
  (absent/empty → `None`), `Severity` (uppercase string → `Severity`).
- `Grype` → parse `.matches[]`, reading `.vulnerability.id`,
  `.artifact.name`, `.artifact.version`, `.vulnerability.fix.versions[0]`
  (fix state `"not-fixed"`/empty → `None`), `.vulnerability.severity`.
- `Custom` → `None`.

Any missing/misshaped field on an individual entry: skip that entry rather
than abort the whole summary. If the top-level structure isn't recognizable at
all (e.g. Trivy's `.Results` absent), return `None` so the caller falls back to
raw. Unknown severity strings map to `Severity::Unknown`.

## Human rendering

New function `pub fn render_summary(result: &ScanResult) -> String`, in
`src/scan_summary.rs` alongside the parsing code.

Successful run with a summary:

```
trivy · nginx:latest
  CRITICAL 3   HIGH 12   MEDIUM 40   LOW 8   UNKNOWN 1   (64 total)

  SEVERITY  CVE               PACKAGE   INSTALLED   FIXED
  CRITICAL  CVE-2025-1234     openssl   3.0.11      3.0.14
  HIGH      CVE-2025-5678     libcurl   8.4.0       8.6.0
  … 49 more (40 medium, 8 low, 1 unknown) — run with --report for full JSON
```

Rules:
- Header: `<tool> · <image ref>`.
- Counts line always lists all five severities and the total.
- Table lists **Critical + High only**, sorted by severity (Critical first)
  then by CVE id. `FIXED` column shows the fixed version or `—` when `None`.
- Footer appears only when lower-severity findings exist; it names the omitted
  counts and points to `--report`.
- Zero vulnerabilities: counts line shows all zeros and the table/footer are
  replaced by a single `No vulnerabilities found.` line.

Fallback cases:
- `summary == null` but the scan ran (custom / unparseable): print the header,
  then `No normalized summary available (custom or unrecognized scanner
  output). Run with --report to see the raw output.`
- Scan failed to run (`error` set, e.g. Trivy not on PATH): print
  `<tool>: <error>` (e.g. `trivy: command not found`) and nothing else. Exit
  stays 0, matching the no-gating philosophy.

## Execution flow

`main.rs`, after `Cli::parse()` and existing validation:

1. If `cli.scan` is set, run `scan::run_scan(...)` → `ScanResult` (unchanged
   from prior spec).
2. Compute `result.summary = scan_summary::summarize(tool, &output)` when
   `output` is present and tool is Trivy/Grype; otherwise `None`.
3. Branch on mode:
   - `--report` set → attach `ScanResult` to the report and print JSON
     (existing path). The `--report`-without-`--scan` and no-flag paths are
     untouched.
   - `--report` not set → `println!("{}", scan_summary::render_summary(&result))`
     and return, before any TUI setup.
4. If `cli.scan` is not set, behavior is exactly as today (TUI or `--report`).

`ScanResult` (in `src/scan.rs`) gains `pub summary: Option<ScanSummary>`,
serialized between `exit_code` and `output`. `run_scan`/`run_resolved_command`
set it to `None`; `main.rs` populates it after the scan so `scan.rs` stays free
of parsing logic (mirrors how `report.rs` stays free of scan logic).

## Implementation shape

- **New `src/scan_summary.rs`:** `Severity`, `Vulnerability`, `SeverityCounts`,
  `ScanSummary` (all `Serialize`); `summarize(tool, &Value) -> Option<ScanSummary>`
  with private `parse_trivy` / `parse_grype`; `render_summary(&ScanResult) -> String`.
  Pure functions — no process spawning, no I/O — so fully unit-testable against
  captured sample JSON.
- **`src/scan.rs`:** add `summary: Option<ScanSummary>` field to `ScanResult`;
  no logic change.
- **`src/main.rs`:** remove `requires = "report"` from the `--scan` arg; add the
  standalone-vs-report branch; populate `result.summary` via `summarize`.
- **`src/report.rs`:** unchanged (already carries `scan: Option<ScanResult>`;
  the new field rides along through serialization).

## Testing

- `summarize` unit tests with small captured Trivy and Grype JSON fixtures:
  correct counts, correct field mapping, `fixed_version: None` when no fix,
  unknown severity → `Unknown`, entries with missing fields skipped,
  unrecognized top-level structure → `None`, `ScanTool::Custom` → `None`.
- `render_summary` unit tests (build `ScanResult` values directly, no
  spawning): Critical+High rows shown and lower severities summarized in the
  footer; zero-vuln "No vulnerabilities found."; `summary: null` fallback
  notice; `error`-set failure line; `FIXED` shows `—` when `fixed_version` is
  `None`.
- `main.rs` CLI tests: `--scan trivy` **without** `--report` now parses
  successfully (the removed-constraint change); existing custom/scan-cmd
  validation tests still pass.

## Documentation

Held to the project's standing bar — README, CLAUDE.md, and `--help` updated in
the same change:

- `README.md`: note the standalone human summary in the `--scan` feature
  bullet; add a "Vulnerability scanning" subsection covering the two modes, the
  `scan.summary` schema, and the Critical+High terminal table; add a jq recipe
  reading `scan.summary.counts`.
- `CLAUDE.md`: update the `--scan` line in `## Core Behavior` to note the
  standalone human summary and the normalized `scan.summary` (and that
  normalization covers Trivy/Grype only).
- `--help` (`EXAMPLES_HELP` in `src/main.rs`): add a standalone `--scan`
  example alongside the existing `--report --scan` one.
- Security note (README, near the `--scan` docs): recommend users run a
  known-good scanner version and verify checksums — Trivy had a supply-chain
  compromise in early 2026 (malicious v0.69.4–v0.69.6 releases). imgchk runs
  whatever `trivy`/`grype` is on `PATH` and does not pin or verify it; the
  trust boundary is the user's installed binary.
