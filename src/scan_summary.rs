use serde::Serialize;

use crate::command_format::strip_control;
use crate::scan::ScanResult;
use crate::scan::ScanTool;

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
            "negligible" => Severity::Low,
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

pub fn summarize(tool: ScanTool, raw: &serde_json::Value) -> Option<ScanSummary> {
    match tool {
        ScanTool::Trivy => parse_trivy(raw),
        ScanTool::Grype => parse_grype(raw),
        ScanTool::Custom => None,
    }
}

fn parse_trivy(raw: &serde_json::Value) -> Option<ScanSummary> {
    // The `Results` key must be present to recognize this as Trivy output.
    let results_val = raw.get("Results")?;
    // Trivy emits `"Results": null` when it ran but found nothing to scan
    // (e.g. no OS/package DB) — a successful empty scan, not a parse failure.
    if results_val.is_null() {
        return Some(ScanSummary::from_vulns(Vec::new()));
    }
    // Present but not an array → genuinely unrecognized structure.
    let results = results_val.as_array()?;
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

    // Sanitize scanner-derived fields once: a malicious image can ship
    // packages with attacker-chosen names/versions, so these strings must not
    // inject terminal escape sequences when printed. Widths and rows both use
    // the sanitized values.
    struct Row {
        severity: &'static str,
        id: String,
        package: String,
        installed: String,
        fixed: String,
    }
    let rows: Vec<Row> = summary
        .vulnerabilities
        .iter()
        .filter(|v| matches!(v.severity, Severity::Critical | Severity::High))
        .map(|v| Row {
            severity: v.severity.label(),
            id: strip_control(&v.id),
            package: strip_control(&v.package),
            installed: strip_control(&v.installed_version),
            fixed: strip_control(v.fixed_version.as_deref().unwrap_or("—")),
        })
        .collect();

    let mut out = format!("{header}\n{counts_line}\n");

    if !rows.is_empty() {
        let id_w = rows
            .iter()
            .map(|r| r.id.len())
            .chain(std::iter::once("CVE".len()))
            .max()
            .unwrap();
        let pkg_w = rows
            .iter()
            .map(|r| r.package.len())
            .chain(std::iter::once("PACKAGE".len()))
            .max()
            .unwrap();
        let inst_w = rows
            .iter()
            .map(|r| r.installed.len())
            .chain(std::iter::once("INSTALLED".len()))
            .max()
            .unwrap();

        out.push('\n');
        out.push_str(&format!(
            "  {:<8}  {:<id_w$}  {:<pkg_w$}  {:<inst_w$}  {}\n",
            "SEVERITY", "CVE", "PACKAGE", "INSTALLED", "FIXED"
        ));
        for r in &rows {
            out.push_str(&format!(
                "  {:<8}  {:<id_w$}  {:<pkg_w$}  {:<inst_w$}  {}\n",
                r.severity, r.id, r.package, r.installed, r.fixed
            ));
        }
    }

    let lower = c.medium + c.low + c.unknown;
    if lower > 0 {
        if rows.is_empty() {
            out.push_str(&format!(
                "  {lower} lower-severity findings ({} medium, {} low, {} unknown) — run with --report for full JSON\n",
                c.medium, c.low, c.unknown
            ));
        } else {
            out.push_str(&format!(
                "  … {lower} more ({} medium, {} low, {} unknown) — run with --report for full JSON\n",
                c.medium, c.low, c.unknown
            ));
        }
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::ScanTool;

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
    fn render_strips_terminal_escapes_from_scanner_fields() {
        // A malicious image can ship packages with attacker-chosen names, so
        // scanner-derived strings must not inject terminal escape sequences.
        let summary = ScanSummary::from_vulns(vec![Vulnerability {
            id: "CVE-2025-\u{1b}[31m1".into(),
            package: "openssl\u{07}".into(),
            installed_version: "3.0\u{1b}]0;x".into(),
            fixed_version: Some("3.1\u{1b}m".into()),
            severity: Severity::Critical,
        }]);
        let out = render_summary("img", &result_with(Some(summary), None));
        assert!(!out.contains('\u{1b}'));
        assert!(!out.contains('\u{07}'));
        // Content still present, just de-escaped.
        assert!(out.contains("openssl"));
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
    fn summarize_trivy_null_results_is_empty_scan_not_failure() {
        // Trivy emits "Results": null when it ran but found nothing to scan
        // (e.g. no OS/package DB on a bare rootfs) — a successful empty scan,
        // not a parse failure. Must return Some(empty), not None.
        let raw = serde_json::json!({ "Results": null });
        let summary = summarize(ScanTool::Trivy, &raw)
            .expect("null Results is a successful empty scan, not a failure");
        assert_eq!(summary.total, 0);
        assert_eq!(summary.counts.critical, 0);
    }

    #[test]
    fn summarize_custom_returns_none() {
        let raw = serde_json::json!({ "Results": [] });
        assert_eq!(summarize(ScanTool::Custom, &raw), None);
    }

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
    fn from_label_negligible_maps_to_low() {
        assert_eq!(Severity::from_label("Negligible"), Severity::Low);
        assert_eq!(Severity::from_label("NEGLIGIBLE"), Severity::Low);
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

    #[test]
    fn render_footer_without_table_omits_more_wording() {
        let summary = ScanSummary::from_vulns(vec![
            Vulnerability {
                id: "CVE-2025-1111".into(),
                package: "zlib".into(),
                installed_version: "1.2".into(),
                fixed_version: None,
                severity: Severity::Medium,
            },
            Vulnerability {
                id: "CVE-2025-2222".into(),
                package: "libpng".into(),
                installed_version: "1.6".into(),
                fixed_version: None,
                severity: Severity::Low,
            },
        ]);
        let out = render_summary("nginx:latest", &result_with(Some(summary), None));
        assert!(!out.contains("more"));
        assert!(out.contains("1 medium"));
        assert!(out.contains("1 low"));
        assert!(out.contains("0 unknown"));
        assert!(out.contains("--report"));
    }
}
