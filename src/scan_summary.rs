#![allow(dead_code)]

use serde::Serialize;

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

fn parse_grype(_raw: &serde_json::Value) -> Option<ScanSummary> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
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
