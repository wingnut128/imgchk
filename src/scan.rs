use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use ocirender::ImageSpec;
use serde::Serialize;
use tempfile::tempdir;

use crate::extract;
use crate::image::LayerInfo;

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

/// Resolve `tool`'s command against `path`, run it via `sh -c`, and capture
/// the result. Exit code 127 ("command not found" — the standard POSIX
/// shell convention) is treated as an imgchk-level error since it means the
/// scanner never actually ran; any other exit code (including non-zero
/// codes scanners use to signal "vulnerabilities found") is a normal run.
pub fn run_resolved_command(tool: ScanTool, custom_cmd: Option<&str>, path: &Path) -> ScanResult {
    let tool_name = tool.name().to_string();
    let command = resolve_command(tool, custom_cmd, path);

    match Command::new("sh").arg("-c").arg(&command).output() {
        Ok(out) => {
            let exit_code = out.status.code();
            if exit_code == Some(127) {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                return ScanResult {
                    tool: tool_name,
                    command,
                    exit_code,
                    output: None,
                    error: Some(if stderr.is_empty() {
                        "command not found".to_string()
                    } else {
                        stderr
                    }),
                };
            }
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let output = serde_json::from_str::<serde_json::Value>(&stdout)
                .unwrap_or(serde_json::Value::String(stdout));
            ScanResult {
                tool: tool_name,
                command,
                exit_code,
                output: Some(output),
                error: None,
            }
        }
        Err(e) => ScanResult {
            tool: tool_name,
            command,
            exit_code: None,
            output: None,
            error: Some(format!("failed to spawn command: {e}")),
        },
    }
}

/// Extract the image's merged (whiteout-resolved) filesystem to a fresh,
/// ephemeral tempdir — independent of `-o`/`cli.output`, always cleaned up
/// after the scan regardless of outcome — then run `tool`'s scan command
/// against it.
pub fn run_scan(tool: ScanTool, custom_cmd: Option<&str>, layers: &[LayerInfo]) -> ScanResult {
    let tool_name = tool.name().to_string();

    let dir = match tempdir() {
        Ok(d) => d,
        Err(e) => {
            return ScanResult {
                tool: tool_name,
                command: String::new(),
                exit_code: None,
                output: None,
                error: Some(format!("failed to create tempdir: {e}")),
            };
        }
    };

    let dir_path: PathBuf = dir.path().to_path_buf();
    if let Err(e) = extract::export_ocirender(layers, ImageSpec::Dir { path: dir_path }) {
        return ScanResult {
            tool: tool_name,
            command: String::new(),
            exit_code: None,
            output: None,
            error: Some(format!("failed to extract image for scanning: {e}")),
        };
    }

    run_resolved_command(tool, custom_cmd, dir.path())
    // `dir` (TempDir) drops at the end of this scope, removing the tempdir.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::FileTree;

    fn bogus_layer() -> LayerInfo {
        LayerInfo {
            index: 0,
            digest: "sha256:deadbeef".to_string(),
            diff_id: "sha256:deadbeef".to_string(),
            size: 0,
            command: "RUN true".to_string(),
            created: "2026-01-01T00:00:00Z".to_string(),
            file_tree: FileTree::new(),
            blob_path: PathBuf::from("/nonexistent/path/does-not-exist.tar.gz"),
            media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
        }
    }

    #[test]
    fn run_scan_reports_error_when_layer_blob_is_missing() {
        let layers = vec![bogus_layer()];
        let result = run_scan(ScanTool::Trivy, None, &layers);

        assert_eq!(result.output, None);
        assert_eq!(result.exit_code, None);
        let error = result.error.expect("expected an error for a missing blob");
        assert!(error.starts_with("failed to extract image for scanning"));
    }

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

    #[test]
    fn run_resolved_command_parses_json_stdout() {
        let dir = std::env::temp_dir();
        let result = run_resolved_command(ScanTool::Custom, Some(r#"echo '{"ok":true}'"#), &dir);

        assert_eq!(result.error, None);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.output, Some(serde_json::json!({"ok": true})));
    }

    #[test]
    fn run_resolved_command_falls_back_to_raw_string_for_non_json_stdout() {
        let dir = std::env::temp_dir();
        let result = run_resolved_command(ScanTool::Custom, Some("echo not-json"), &dir);

        assert_eq!(result.error, None);
        let output = result.output.expect("expected raw string output");
        assert!(output.as_str().unwrap().contains("not-json"));
    }

    #[test]
    fn run_resolved_command_reports_error_for_missing_command() {
        let dir = std::env::temp_dir();
        let result = run_resolved_command(
            ScanTool::Custom,
            Some("definitely-not-a-real-command-xyz123"),
            &dir,
        );

        assert_eq!(result.output, None);
        assert_eq!(result.exit_code, Some(127));
        assert!(result.error.is_some());
    }
}
