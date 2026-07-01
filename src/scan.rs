use std::path::Path;

use serde::Serialize;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanTool {
    Trivy,
    Grype,
    Custom,
}

impl ScanTool {
    pub fn name(self) -> &'static str {
        match self {
            ScanTool::Trivy => "trivy",
            ScanTool::Grype => "grype",
            ScanTool::Custom => "custom",
        }
    }
}

#[derive(Serialize)]
pub struct ScanResult {
    pub tool: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// Build the fully-resolved shell command for `tool`, substituting every
/// `{path}` occurrence with `path`. `custom_cmd` is required (and used)
/// only for `ScanTool::Custom` — CLI validation guarantees it's `Some` by
/// the time this is called with `ScanTool::Custom`.
pub fn resolve_command(tool: ScanTool, custom_cmd: Option<&str>, path: &Path) -> String {
    let template = match tool {
        ScanTool::Trivy => "trivy rootfs --format json {path}",
        ScanTool::Grype => "grype dir:{path} -o json",
        ScanTool::Custom => {
            custom_cmd.expect("ScanTool::Custom requires custom_cmd (enforced by CLI validation)")
        }
    };
    template.replace("{path}", &path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_command_trivy_preset() {
        let cmd = resolve_command(ScanTool::Trivy, None, Path::new("/tmp/scan-xyz"));
        assert_eq!(cmd, "trivy rootfs --format json /tmp/scan-xyz");
    }

    #[test]
    fn resolve_command_grype_preset() {
        let cmd = resolve_command(ScanTool::Grype, None, Path::new("/tmp/scan-xyz"));
        assert_eq!(cmd, "grype dir:/tmp/scan-xyz -o json");
    }

    #[test]
    fn resolve_command_custom_substitutes_path() {
        let cmd = resolve_command(
            ScanTool::Custom,
            Some("mytool scan {path} --json"),
            Path::new("/tmp/scan-xyz"),
        );
        assert_eq!(cmd, "mytool scan /tmp/scan-xyz --json");
    }

    #[test]
    fn resolve_command_custom_substitutes_multiple_placeholders() {
        let cmd = resolve_command(
            ScanTool::Custom,
            Some("cd {path} && mytool scan {path}"),
            Path::new("/tmp/scan-xyz"),
        );
        assert_eq!(cmd, "cd /tmp/scan-xyz && mytool scan /tmp/scan-xyz");
    }
}
