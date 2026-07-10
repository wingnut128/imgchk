#![allow(dead_code)]

use serde::Serialize;

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
