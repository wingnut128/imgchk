# Friendlier `--scan` Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `--scan` a normalized `scan.summary` in `--report` JSON and a human-readable terminal summary when `--scan` runs without `--report`.

**Architecture:** A new pure-function module `src/scan_summary.rs` parses Trivy/Grype JSON into a tool-agnostic `ScanSummary` and renders a compact terminal view. `ScanResult` gains a `summary` field; `main.rs` populates it after the scan and branches between JSON (`--report`) and human output. `src/scan.rs` (process spawning) and `src/report.rs` (report shape) stay free of parsing logic.

**Tech Stack:** Rust 2021, serde/serde_json, clap. Tests are in-file `#[cfg(test)] mod tests` (the codebase's existing pattern).

## Global Constraints

- Normalize **Trivy and Grype only**. `ScanTool::Custom` and any unrecognized/unparseable output yield `summary: None`; raw `scan.output` is always retained.
- Skip individual malformed entries rather than aborting a whole summary; return `None` only when the top-level structure is unrecognizable.
- No exit-code gating in either mode — imgchk exits 0 even when the scanner found vulnerabilities or wasn't found.
- Terminal table lists **Critical + High only**; lower severities are a footer count. Counts line always lists all five severities.
- Every CLI-surface change updates `README.md`, `CLAUDE.md`, and `--help` in the same change (project standing rule).
- Pre-commit hook runs `cargo fmt --check` + `cargo clippy` — code must be clean before each commit.

---

### Task 1: Summary data types

**Files:**
- Create: `src/scan_summary.rs`
- Modify: `src/main.rs` (add `mod scan_summary;` alongside the other `mod` lines near the top, e.g. after `mod scan;`)
- Test: in `src/scan_summary.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `Severity` (enum: `Critical`, `High`, `Medium`, `Low`, `Unknown`; `Ord` with `Critical` least so ascending sort puts it first; `Serialize` lowercase), `Severity::from_label(&str) -> Severity`, `Severity::label(self) -> &'static str`; `SeverityCounts { critical, high, medium, low, unknown: usize }` with `increment(&mut self, Severity)`; `Vulnerability { id, package, installed_version: String, fixed_version: Option<String>, severity: Severity }`; `ScanSummary { counts: SeverityCounts, total: usize, vulnerabilities: Vec<Vulnerability> }` with `ScanSummary::from_vulns(Vec<Vulnerability>) -> ScanSummary`.

- [ ] **Step 1: Write the failing tests**

Create `src/scan_summary.rs` with the types-under-test stubbed just enough to compile is NOT the approach — write the tests first against the intended API, then add the module to `main.rs` so it compiles. Put this in `src/scan_summary.rs`:

```rust
use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_label_is_case_insensitive() {
        assert_eq!(Severity::from_label("CRITICAL"), Severity::Critical);
        assert_eq!(Severity::from_label("Critical"), Severity::Critical);
        assert_eq!(Severity::from_label("high"), Severity::High);
    }

    #[test]
    fn from_label_unknown_maps_to_unknown() {
        assert_eq!(Severity::from_label("bogus"), Severity::Unknown);
        assert_eq!(Severity::from_label(""), Severity::Unknown);
    }

    #[test]
    fn severity_serializes_lowercase() {
        let json = serde_json::to_string(&Severity::Critical).unwrap();
        assert_eq!(json, "\"critical\"");
    }

    #[test]
    fn critical_sorts_before_high() {
        assert!(Severity::Critical < Severity::High);
        assert!(Severity::High < Severity::Unknown);
    }

    #[test]
    fn from_vulns_counts_totals_and_sorts() {
        let vulns = vec![
            Vulnerability {
                id: "CVE-2".into(),
                package: "b".into(),
                installed_version: "1".into(),
                fixed_version: None,
                severity: Severity::Low,
            },
            Vulnerability {
                id: "CVE-1".into(),
                package: "a".into(),
                installed_version: "1".into(),
                fixed_version: Some("2".into()),
                severity: Severity::Critical,
            },
            Vulnerability {
                id: "CVE-3".into(),
                package: "c".into(),
                installed_version: "1".into(),
                fixed_version: None,
                severity: Severity::Critical,
            },
        ];
        let summary = ScanSummary::from_vulns(vulns);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.counts.critical, 2);
        assert_eq!(summary.counts.low, 1);
        // Sorted: Critical first, then by id ascending.
        assert_eq!(summary.vulnerabilities[0].id, "CVE-1");
        assert_eq!(summary.vulnerabilities[1].id, "CVE-3");
        assert_eq!(summary.vulnerabilities[2].id, "CVE-2");
    }
}
```

Add `mod scan_summary;` to `src/main.rs` (near the existing `mod scan;`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test scan_summary 2>&1 | tail -20`
Expected: FAIL — `cannot find type Severity` / `Vulnerability` / `ScanSummary` in this scope.

- [ ] **Step 3: Write the implementation**

Add above the `#[cfg(test)]` block in `src/scan_summary.rs`:

```rust
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Unknown,
}

impl Severity {
    pub fn from_label(s: &str) -> Severity {
        match s.to_ascii_lowercase().as_str() {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" => Severity::Medium,
            "low" => Severity::Low,
            _ => Severity::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Critical => "CRITICAL",
            Severity::High => "HIGH",
            Severity::Medium => "MEDIUM",
            Severity::Low => "LOW",
            Severity::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct SeverityCounts {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub unknown: usize,
}

impl SeverityCounts {
    fn increment(&mut self, severity: Severity) {
        match severity {
            Severity::Critical => self.critical += 1,
            Severity::High => self.high += 1,
            Severity::Medium => self.medium += 1,
            Severity::Low => self.low += 1,
            Severity::Unknown => self.unknown += 1,
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Vulnerability {
    pub id: String,
    pub package: String,
    pub installed_version: String,
    pub fixed_version: Option<String>,
    pub severity: Severity,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ScanSummary {
    pub counts: SeverityCounts,
    pub total: usize,
    pub vulnerabilities: Vec<Vulnerability>,
}

impl ScanSummary {
    pub fn from_vulns(mut vulns: Vec<Vulnerability>) -> ScanSummary {
        vulns.sort_by(|a, b| a.severity.cmp(&b.severity).then_with(|| a.id.cmp(&b.id)));
        let mut counts = SeverityCounts::default();
        for v in &vulns {
            counts.increment(v.severity);
        }
        let total = vulns.len();
        ScanSummary {
            counts,
            total,
            vulnerabilities: vulns,
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test scan_summary 2>&1 | tail -20`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/scan_summary.rs src/main.rs
git commit -m "feat: add scan summary data types (Severity, Vulnerability, ScanSummary)"
```

---

### Task 2: Trivy parser + summarize dispatch

**Files:**
- Modify: `src/scan_summary.rs`
- Test: `src/scan_summary.rs` (`mod tests`)

**Interfaces:**
- Consumes: `crate::scan::ScanTool` (`Trivy`, `Grype`, `Custom`); `ScanSummary::from_vulns`, `Vulnerability`, `Severity` from Task 1.
- Produces: `pub fn summarize(tool: ScanTool, raw: &serde_json::Value) -> Option<ScanSummary>`; private `fn parse_trivy(raw: &serde_json::Value) -> Option<ScanSummary>`.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/scan_summary.rs`:

```rust
    use crate::scan::ScanTool;

    #[test]
    fn summarize_trivy_maps_fields_and_counts() {
        let raw = serde_json::json!({
            "Results": [{
                "Vulnerabilities": [
                    {
                        "VulnerabilityID": "CVE-2025-1234",
                        "PkgName": "openssl",
                        "InstalledVersion": "3.0.11",
                        "FixedVersion": "3.0.14",
                        "Severity": "CRITICAL"
                    },
                    {
                        "VulnerabilityID": "CVE-2025-5678",
                        "PkgName": "libcurl",
                        "InstalledVersion": "8.4.0",
                        "FixedVersion": "",
                        "Severity": "HIGH"
                    }
                ]
            }]
        });
        let summary = summarize(ScanTool::Trivy, &raw).expect("trivy should parse");
        assert_eq!(summary.total, 2);
        assert_eq!(summary.counts.critical, 1);
        assert_eq!(summary.counts.high, 1);
        let crit = &summary.vulnerabilities[0];
        assert_eq!(crit.id, "CVE-2025-1234");
        assert_eq!(crit.package, "openssl");
        assert_eq!(crit.installed_version, "3.0.11");
        assert_eq!(crit.fixed_version.as_deref(), Some("3.0.14"));
        // Empty FixedVersion -> None.
        let high = &summary.vulnerabilities[1];
        assert_eq!(high.fixed_version, None);
    }

    #[test]
    fn summarize_trivy_skips_entries_missing_required_fields() {
        let raw = serde_json::json!({
            "Results": [{
                "Vulnerabilities": [
                    { "PkgName": "x", "InstalledVersion": "1", "Severity": "LOW" }
                ]
            }]
        });
        let summary = summarize(ScanTool::Trivy, &raw).expect("still a recognized shape");
        assert_eq!(summary.total, 0);
    }

    #[test]
    fn summarize_trivy_unknown_severity_maps_to_unknown() {
        let raw = serde_json::json!({
            "Results": [{
                "Vulnerabilities": [
                    { "VulnerabilityID": "CVE-9", "PkgName": "p", "InstalledVersion": "1", "Severity": "WEIRD" }
                ]
            }]
        });
        let summary = summarize(ScanTool::Trivy, &raw).unwrap();
        assert_eq!(summary.counts.unknown, 1);
    }

    #[test]
    fn summarize_trivy_unrecognized_structure_returns_none() {
        let raw = serde_json::json!({ "nope": true });
        assert_eq!(summarize(ScanTool::Trivy, &raw), None);
    }

    #[test]
    fn summarize_custom_returns_none() {
        let raw = serde_json::json!({ "Results": [] });
        assert_eq!(summarize(ScanTool::Custom, &raw), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test scan_summary::tests::summarize 2>&1 | tail -20`
Expected: FAIL — `cannot find function summarize in this scope`.

- [ ] **Step 3: Write the implementation**

Add to `src/scan_summary.rs` (below the type impls, above `#[cfg(test)]`). Add `use crate::scan::ScanTool;` to the top-of-file `use` lines:

```rust
pub fn summarize(tool: ScanTool, raw: &serde_json::Value) -> Option<ScanSummary> {
    match tool {
        ScanTool::Trivy => parse_trivy(raw),
        ScanTool::Grype => parse_grype(raw),
        ScanTool::Custom => None,
    }
}

fn parse_trivy(raw: &serde_json::Value) -> Option<ScanSummary> {
    let results = raw.get("Results")?.as_array()?;
    let mut vulns = Vec::new();
    for result in results {
        let Some(entries) = result.get("Vulnerabilities").and_then(|v| v.as_array()) else {
            continue;
        };
        for entry in entries {
            let (Some(id), Some(package), Some(installed)) = (
                entry.get("VulnerabilityID").and_then(|v| v.as_str()),
                entry.get("PkgName").and_then(|v| v.as_str()),
                entry.get("InstalledVersion").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            let fixed_version = entry
                .get("FixedVersion")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            let severity = entry
                .get("Severity")
                .and_then(|v| v.as_str())
                .map(Severity::from_label)
                .unwrap_or(Severity::Unknown);
            vulns.push(Vulnerability {
                id: id.to_string(),
                package: package.to_string(),
                installed_version: installed.to_string(),
                fixed_version,
                severity,
            });
        }
    }
    Some(ScanSummary::from_vulns(vulns))
}
```

Note: `parse_grype` is referenced here but defined in Task 3. To keep this task compiling on its own, add a temporary stub now and replace it in Task 3:

```rust
fn parse_grype(_raw: &serde_json::Value) -> Option<ScanSummary> {
    None
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test scan_summary 2>&1 | tail -20`
Expected: PASS (10 tests total).

- [ ] **Step 5: Commit**

```bash
git add src/scan_summary.rs
git commit -m "feat: parse Trivy JSON into normalized scan summary"
```

---

### Task 3: Grype parser

**Files:**
- Modify: `src/scan_summary.rs` (replace the `parse_grype` stub)
- Test: `src/scan_summary.rs` (`mod tests`)

**Interfaces:**
- Consumes: same as Task 2.
- Produces: real `fn parse_grype(raw: &serde_json::Value) -> Option<ScanSummary>`.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
    #[test]
    fn summarize_grype_maps_fields_and_fix_versions() {
        let raw = serde_json::json!({
            "matches": [
                {
                    "vulnerability": {
                        "id": "CVE-2025-1234",
                        "severity": "Critical",
                        "fix": { "versions": ["3.0.14"], "state": "fixed" }
                    },
                    "artifact": { "name": "openssl", "version": "3.0.11" }
                },
                {
                    "vulnerability": {
                        "id": "CVE-2025-5678",
                        "severity": "High",
                        "fix": { "versions": [], "state": "not-fixed" }
                    },
                    "artifact": { "name": "libcurl", "version": "8.4.0" }
                }
            ]
        });
        let summary = summarize(ScanTool::Grype, &raw).expect("grype should parse");
        assert_eq!(summary.total, 2);
        assert_eq!(summary.counts.critical, 1);
        let crit = &summary.vulnerabilities[0];
        assert_eq!(crit.id, "CVE-2025-1234");
        assert_eq!(crit.package, "openssl");
        assert_eq!(crit.installed_version, "3.0.11");
        assert_eq!(crit.fixed_version.as_deref(), Some("3.0.14"));
        // Empty fix versions -> None.
        assert_eq!(summary.vulnerabilities[1].fixed_version, None);
    }

    #[test]
    fn summarize_grype_unrecognized_structure_returns_none() {
        let raw = serde_json::json!({ "Results": [] });
        assert_eq!(summarize(ScanTool::Grype, &raw), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test scan_summary::tests::summarize_grype 2>&1 | tail -20`
Expected: FAIL — `summarize_grype_maps_fields_and_fix_versions` fails (stub returns `None`, so `.expect("grype should parse")` panics).

- [ ] **Step 3: Write the implementation**

Replace the `parse_grype` stub in `src/scan_summary.rs` with:

```rust
fn parse_grype(raw: &serde_json::Value) -> Option<ScanSummary> {
    let matches = raw.get("matches")?.as_array()?;
    let mut vulns = Vec::new();
    for entry in matches {
        let Some(vuln) = entry.get("vulnerability") else {
            continue;
        };
        let Some(artifact) = entry.get("artifact") else {
            continue;
        };
        let (Some(id), Some(package), Some(installed)) = (
            vuln.get("id").and_then(|v| v.as_str()),
            artifact.get("name").and_then(|v| v.as_str()),
            artifact.get("version").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let fixed_version = vuln
            .get("fix")
            .and_then(|f| f.get("versions"))
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        let severity = vuln
            .get("severity")
            .and_then(|v| v.as_str())
            .map(Severity::from_label)
            .unwrap_or(Severity::Unknown);
        vulns.push(Vulnerability {
            id: id.to_string(),
            package: package.to_string(),
            installed_version: installed.to_string(),
            fixed_version,
            severity,
        });
    }
    Some(ScanSummary::from_vulns(vulns))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test scan_summary 2>&1 | tail -20`
Expected: PASS (12 tests total).

- [ ] **Step 5: Commit**

```bash
git add src/scan_summary.rs
git commit -m "feat: parse Grype JSON into normalized scan summary"
```

---

### Task 4: `ScanResult.summary` field + human renderer

**Files:**
- Modify: `src/scan.rs` (add `summary` field to `ScanResult`; set `summary: None` in all five `ScanResult` literals)
- Modify: `src/scan_summary.rs` (add `render_summary`)
- Test: `src/scan_summary.rs` (`mod tests`)

**Interfaces:**
- Consumes: `crate::scan::ScanResult` (now with `pub summary: Option<ScanSummary>`); `Severity`, `Vulnerability`, `ScanSummary` from Task 1.
- Produces: `pub fn render_summary(image_ref: &str, result: &ScanResult) -> String`.

- [ ] **Step 1: Add the `summary` field to `ScanResult`**

In `src/scan.rs`, add `use crate::scan_summary::ScanSummary;` to the top `use` lines, and update the struct (field between `exit_code` and `output`):

```rust
#[derive(Serialize)]
pub struct ScanResult {
    pub tool: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub summary: Option<ScanSummary>,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}
```

Then add `summary: None,` to **all five** `ScanResult { ... }` literals in `src/scan.rs`:
- `run_resolved_command`: the `exit_code == Some(127)` branch, the normal-return branch, and the spawn-error (`Err(e)`) branch.
- `run_scan`: the `tempdir()` error branch and the `export_ocirender` error branch.

- [ ] **Step 2: Verify existing scan tests still compile and pass**

Run: `cargo test scan:: 2>&1 | tail -20`
Expected: PASS — the existing `scan::tests` still pass (the new field defaults to `None` and they don't assert on it).

- [ ] **Step 3: Write the failing renderer tests**

Add to `mod tests` in `src/scan_summary.rs`:

```rust
    use crate::scan::ScanResult;

    fn result_with(summary: Option<ScanSummary>, error: Option<String>) -> ScanResult {
        ScanResult {
            tool: "trivy".to_string(),
            command: "trivy rootfs --format json /tmp/x".to_string(),
            exit_code: Some(0),
            summary,
            output: Some(serde_json::json!({"raw": true})),
            error,
        }
    }

    #[test]
    fn render_shows_critical_high_rows_and_footer() {
        let summary = ScanSummary::from_vulns(vec![
            Vulnerability {
                id: "CVE-2025-1234".into(),
                package: "openssl".into(),
                installed_version: "3.0.11".into(),
                fixed_version: Some("3.0.14".into()),
                severity: Severity::Critical,
            },
            Vulnerability {
                id: "CVE-2025-9999".into(),
                package: "zlib".into(),
                installed_version: "1.2".into(),
                fixed_version: None,
                severity: Severity::Medium,
            },
        ]);
        let out = render_summary("nginx:latest", &result_with(Some(summary), None));
        assert!(out.contains("trivy · nginx:latest"));
        assert!(out.contains("CRITICAL 1"));
        assert!(out.contains("CVE-2025-1234"));
        assert!(out.contains("openssl"));
        // Medium is not in the table but is summarized in the footer.
        assert!(!out.contains("CVE-2025-9999"));
        assert!(out.contains("1 more"));
        assert!(out.contains("--report"));
    }

    #[test]
    fn render_missing_fix_shows_dash() {
        let summary = ScanSummary::from_vulns(vec![Vulnerability {
            id: "CVE-1".into(),
            package: "p".into(),
            installed_version: "1".into(),
            fixed_version: None,
            severity: Severity::High,
        }]);
        let out = render_summary("img", &result_with(Some(summary), None));
        assert!(out.contains("—"));
    }

    #[test]
    fn render_zero_vulns() {
        let summary = ScanSummary::from_vulns(vec![]);
        let out = render_summary("img", &result_with(Some(summary), None));
        assert!(out.contains("No vulnerabilities found."));
    }

    #[test]
    fn render_no_summary_fallback_notice() {
        let out = render_summary("img", &result_with(None, None));
        assert!(out.contains("No normalized summary available"));
    }

    #[test]
    fn render_error_line() {
        let out = render_summary("img", &result_with(None, Some("command not found".into())));
        assert_eq!(out, "trivy: command not found");
    }
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test scan_summary::tests::render 2>&1 | tail -20`
Expected: FAIL — `cannot find function render_summary in this scope`.

- [ ] **Step 5: Write the renderer**

Add to `src/scan_summary.rs` (below `summarize`, above `#[cfg(test)]`). Add `use crate::scan::ScanResult;` to the top-of-file `use` lines:

```rust
pub fn render_summary(image_ref: &str, result: &ScanResult) -> String {
    if let Some(err) = &result.error {
        return format!("{}: {}", result.tool, err);
    }

    let header = format!("{} · {}", result.tool, image_ref);

    let Some(summary) = &result.summary else {
        return format!(
            "{header}\n  No normalized summary available (custom or unrecognized \
             scanner output). Run with --report to see the raw output."
        );
    };

    let c = &summary.counts;
    let counts_line = format!(
        "  CRITICAL {}   HIGH {}   MEDIUM {}   LOW {}   UNKNOWN {}   ({} total)",
        c.critical, c.high, c.medium, c.low, c.unknown, summary.total
    );

    if summary.total == 0 {
        return format!("{header}\n{counts_line}\n  No vulnerabilities found.");
    }

    let shown: Vec<&Vulnerability> = summary
        .vulnerabilities
        .iter()
        .filter(|v| matches!(v.severity, Severity::Critical | Severity::High))
        .collect();

    let mut out = format!("{header}\n{counts_line}\n");

    if !shown.is_empty() {
        let id_w = shown
            .iter()
            .map(|v| v.id.len())
            .chain(std::iter::once("CVE".len()))
            .max()
            .unwrap();
        let pkg_w = shown
            .iter()
            .map(|v| v.package.len())
            .chain(std::iter::once("PACKAGE".len()))
            .max()
            .unwrap();
        let inst_w = shown
            .iter()
            .map(|v| v.installed_version.len())
            .chain(std::iter::once("INSTALLED".len()))
            .max()
            .unwrap();

        out.push('\n');
        out.push_str(&format!(
            "  {:<8}  {:<id_w$}  {:<pkg_w$}  {:<inst_w$}  {}\n",
            "SEVERITY", "CVE", "PACKAGE", "INSTALLED", "FIXED"
        ));
        for v in &shown {
            let fixed = v.fixed_version.as_deref().unwrap_or("—");
            out.push_str(&format!(
                "  {:<8}  {:<id_w$}  {:<pkg_w$}  {:<inst_w$}  {}\n",
                v.severity.label(),
                v.id,
                v.package,
                v.installed_version,
                fixed
            ));
        }
    }

    let lower = c.medium + c.low + c.unknown;
    if lower > 0 {
        out.push_str(&format!(
            "  … {lower} more ({} medium, {} low, {} unknown) — run with --report for full JSON\n",
            c.medium, c.low, c.unknown
        ));
    }

    out.trim_end().to_string()
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test scan_summary 2>&1 | tail -20`
Expected: PASS (17 tests total).

- [ ] **Step 7: Commit**

```bash
git add src/scan.rs src/scan_summary.rs
git commit -m "feat: add scan.summary field and human-readable render_summary"
```

---

### Task 5: Wire modes into `main.rs`

**Files:**
- Modify: `src/main.rs` (drop `requires = "report"`; update the doc comment; add the scan branch; update the CLI test)
- Test: `src/main.rs` (`mod tests`)

**Interfaces:**
- Consumes: `scan::run_scan`, `scan_summary::summarize`, `scan_summary::render_summary`, `report::build_report`.
- Produces: no new public API; the two output modes.

- [ ] **Step 1: Update the CLI test to expect standalone `--scan`**

In `src/main.rs` `mod tests`, replace the existing `cli_scan_requires_report` test with:

```rust
    #[test]
    fn cli_scan_standalone_parses() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest", "--scan", "trivy"]);
        assert_eq!(cli.scan, Some(scan::ScanTool::Trivy));
        assert!(!cli.report);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test cli_scan_standalone_parses 2>&1 | tail -20`
Expected: FAIL — `Cli::parse_from` panics because clap still enforces `requires = "report"`.

- [ ] **Step 3: Drop the constraint and update docs on the arg**

In `src/main.rs`, change the `scan` field attribute and doc comment:

```rust
    /// Run an external scanner against the merged image filesystem
    /// (trivy, grype, or custom). Without --report, prints a human-readable
    /// summary; with --report, embeds a normalized summary in the JSON.
    #[arg(long, value_enum)]
    scan: Option<scan::ScanTool>,
```

Update the `validate_scan_args` doc comment to drop the now-false sentence about `requires = "report"`:

```rust
/// Cross-flag rules clap's declarative attributes can't express (they
/// depend on `scan`'s specific value, not just presence).
fn validate_scan_args(cli: &Cli) -> anyhow::Result<()> {
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test cli_scan_standalone_parses 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Add the scan branch in `main()`**

In `src/main.rs`, replace the existing `if cli.report { ... }` block (the one that builds the report and conditionally calls `scan::run_scan`) with:

```rust
    if let Some(tool) = cli.scan {
        let mut result = scan::run_scan(tool, cli.scan_cmd.as_deref(), &image.layers);
        if let Some(output) = &result.output {
            result.summary = scan_summary::summarize(tool, output);
        }
        if cli.report {
            let mut report = report::build_report(&image);
            report.scan = Some(result);
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("{}", scan_summary::render_summary(image_ref, &result));
        }
        return Ok(());
    }

    if cli.report {
        let report = report::build_report(&image);
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
```

- [ ] **Step 6: Run the full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: PASS — all tests (including the existing `custom`/`scan-cmd` validation tests) pass.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat: --scan without --report prints human summary; embed normalized summary in --report"
```

---

### Task 6: Documentation + end-to-end verification

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `src/main.rs` (`EXAMPLES_HELP`)

**Interfaces:**
- Consumes: the finished feature.
- Produces: docs only.

- [ ] **Step 1: Verify the human mode end-to-end (standalone, custom fallback)**

Run:
```bash
cargo run -- alpine:3.19 --scan custom --scan-cmd 'echo {}'
```
Expected: prints `custom · alpine:3.19` followed by the "No normalized summary available …" notice (custom scanners aren't normalized), and exits 0. This confirms the non-`--report` branch prints `render_summary` output.

- [ ] **Step 2: Verify the JSON mode carries `summary: null` for custom**

Run:
```bash
cargo run -- alpine:3.19 --report --scan custom --scan-cmd 'echo {}' | jq '.scan | {tool, summary, has_output: (.output != null)}'
```
Expected: `tool: "custom"`, `summary: null`, `has_output: true`.

- [ ] **Step 3 (optional): Verify against real Trivy if installed**

Run (skip if `trivy` isn't on PATH):
```bash
command -v trivy && cargo run -- alpine:3.19 --scan trivy
```
Expected: header, a counts line, a Critical+High table (if any), and a footer for lower severities.

- [ ] **Step 4: Update `README.md`**

- In the `## Features` list, extend the `--scan` bullet to mention: standalone `--scan` prints a human-readable summary; `--report --scan` embeds a normalized `scan.summary`.
- In the scanning subsection, document the two modes, the `scan.summary` schema (`counts`, `total`, `vulnerabilities[]` with `id`/`package`/`installed_version`/`fixed_version`/`severity`), that normalization covers **Trivy and Grype only** (custom → `summary: null`, raw retained), and that the terminal table shows Critical + High with lower severities in a footer.
- Add a jq recipe:

```bash
# Severity counts from a scan
imgchk nginx:latest --report --scan trivy | jq '.scan.summary.counts'
```

- Add a short **security note** near the `--scan` docs: recommend running a known-good scanner version and verifying checksums — Trivy had a supply-chain compromise in early 2026 (malicious v0.69.4–v0.69.6). imgchk runs whatever `trivy`/`grype` is on `PATH` and does not pin or verify it; the trust boundary is the user's installed binary.

- [ ] **Step 5: Update `CLAUDE.md`**

Update item 6 (Vulnerability scanning) under `## Core Behavior` to note: without `--report`, `--scan` prints a human-readable summary (severity counts + Critical/High table); with `--report`, it embeds a normalized `scan.summary` alongside the raw output; normalization covers Trivy/Grype only (`custom` → `summary: null`). Implemented in `src/scan_summary.rs`.

- [ ] **Step 6: Update `--help` (`EXAMPLES_HELP` in `src/main.rs`)**

Add a standalone example after the existing `--report --scan` examples:

```
    Print a human-readable vulnerability summary (no --report needed):
        imgchk nginx:latest --scan trivy
```

- [ ] **Step 7: Confirm build + help render, then commit**

Run:
```bash
cargo build --release 2>&1 | tail -3 && cargo run -- --help | grep -A1 "human-readable vulnerability"
```
Expected: build succeeds; the new example appears in `--help`.

```bash
git add README.md CLAUDE.md src/main.rs
git commit -m "docs: document standalone --scan summary and normalized scan.summary"
```

---

## Self-Review

**Spec coverage:**
- Standalone-vs-report invocation (drop `requires`) → Task 5. ✓
- Normalized `ScanSummary` shape (Severity, Vulnerability, SeverityCounts, counts/total/sorted list) → Tasks 1–3. ✓
- Trivy + Grype parsers, custom/unparseable → `None`, skip malformed entries, unrecognized top-level → `None` → Tasks 2–3. ✓
- Raw `output` retained + `summary` added between `exit_code` and `output` → Task 4. ✓
- Human rendering: header, counts line, Critical+High table, `—` for missing fix, footer, zero-vuln line, custom/unparseable fallback, scan-failed error line → Task 4. ✓
- Execution flow / `main.rs` branch, `summary` populated from `output` → Task 5. ✓
- Testing (summarize + render unit tests, updated CLI test) → Tasks 1–5. ✓
- Docs (README/CLAUDE.md/`--help`) + security note → Task 6. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. The one intentional stub (`parse_grype` in Task 2) is explicitly flagged and replaced in Task 3. ✓

**Type consistency:** `summarize(tool, &Value) -> Option<ScanSummary>`, `render_summary(&str, &ScanResult) -> String`, `ScanSummary::from_vulns(Vec<Vulnerability>)`, `Severity::from_label`/`label`, `ScanResult.summary` field — names/signatures match across Tasks 1–5. `image_ref` is the existing `&str` binding in `main()`. ✓
