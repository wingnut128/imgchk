mod action;
mod command_format;
mod extract;
mod image;
mod report;
mod scan;
mod scan_summary;
mod selection;
mod tree;
mod ui;
mod update;
mod view;

use std::path::PathBuf;

use clap::{CommandFactory, Parser};

const EXAMPLES_HELP: &str = "\
EXAMPLES:
    Inspect an image from Docker Hub:
        imgchk nginx:latest

    Inspect with a specific platform:
        imgchk --platform linux/arm64 alpine:3.19

    Inspect a local tarball (from `docker save`):
        imgchk ./myimage.tar

    Set an output directory for extractions:
        imgchk -o /tmp/extracted ghcr.io/org/app:v1.2

    Print a JSON report instead of launching the TUI:
        imgchk nginx:latest --report

    Run trivy against the image and embed the results in the report:
        imgchk nginx:latest --report --scan trivy

    Use any scanner via a custom command ({path} is the extracted rootfs dir):
        imgchk nginx:latest --report --scan custom --scan-cmd 'mytool scan {path} --json'

    Print a human-readable vulnerability summary (no --report needed):
        imgchk nginx:latest --scan trivy

TUI KEYBINDINGS:
    j/k, Up/Down    Navigate layers or files
    Tab             Cycle pane focus (Layers → Files → Details)
    Enter           Expand/collapse directory in file tree
    Space           Select/deselect file or directory
    t               Toggle layer view / cumulative view
    f               Cycle export format (tar.gz, tar, squashfs, dir)
    o               Set output directory
    e               Extract (files or current layer)
    a               Export all layers
    q               Quit

ENVIRONMENT:
    IMGCHK_REGISTRY_USER    Registry username
    IMGCHK_REGISTRY_TOKEN   Registry password/token
    IMGCHK_CACHE_DIR        Override blob cache directory (~/.cache/imgchk/blobs/)
    IMGCHK_CACHE_MAX_MB     Max cache size in MB (default: 10240)";

/// Interactive TUI for inspecting OCI/Docker container images.
///
/// Fetches image manifests and layers from registries or local tarballs,
/// displays layer metadata and file trees, and supports extraction in
/// multiple formats (tar.gz, tar, squashfs, directory).
#[derive(Parser)]
#[command(
    name = "imgchk",
    version,
    about = "Container image inspector and layer extraction tool",
    after_long_help = EXAMPLES_HELP,
)]
struct Cli {
    /// Image reference (e.g., nginx:latest) or path to a tarball
    image: Option<String>,

    /// Output directory for extracted files/layers
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Target platform (e.g., linux/amd64, linux/arm64)
    #[arg(long, default_value = "linux/amd64")]
    platform: String,

    /// Print a JSON analysis report to stdout instead of launching the TUI
    #[arg(long)]
    report: bool,

    /// Run an external scanner against the merged image filesystem
    /// (trivy, grype, or custom). Without --report, prints a human-readable
    /// summary; with --report, embeds a normalized summary in the JSON.
    #[arg(long, value_enum)]
    scan: Option<scan::ScanTool>,

    /// Custom scanner command template, required iff --scan=custom.
    /// Use {path} as a placeholder for the extracted rootfs directory.
    #[arg(long)]
    scan_cmd: Option<String>,
}

/// Cross-flag rules clap's declarative attributes can't express (they
/// depend on `scan`'s specific value, not just presence).
fn validate_scan_args(cli: &Cli) -> anyhow::Result<()> {
    if cli.scan == Some(scan::ScanTool::Custom) && cli.scan_cmd.is_none() {
        anyhow::bail!("--scan=custom requires --scan-cmd");
    }
    if cli.scan != Some(scan::ScanTool::Custom) && cli.scan_cmd.is_some() {
        anyhow::bail!("--scan-cmd is only valid with --scan=custom");
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    validate_scan_args(&cli)?;

    let image_ref = match cli.image {
        Some(ref img) => img.as_str(),
        None => {
            Cli::command().print_long_help()?;
            println!();
            std::process::exit(0);
        }
    };

    use image::ImageSource;
    let rt = tokio::runtime::Runtime::new()?;
    let image = if image::is_tarball(image_ref) {
        rt.block_on(image::TarballSource.load(image_ref, Some(&cli.platform)))?
    } else {
        rt.block_on(image::RegistrySource::default().load(image_ref, Some(&cli.platform)))?
    };

    if image.layers.is_empty() {
        anyhow::bail!("No layers found in image");
    }

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

    ui::run(image, cli.output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_report_flag() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest", "--report"]);
        assert!(cli.report);
        assert_eq!(cli.image.as_deref(), Some("nginx:latest"));
    }

    #[test]
    fn cli_report_defaults_to_false() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest"]);
        assert!(!cli.report);
    }

    #[test]
    fn cli_scan_standalone_parses() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest", "--scan", "trivy"]);
        assert_eq!(cli.scan, Some(scan::ScanTool::Trivy));
        assert!(!cli.report);
    }

    #[test]
    fn cli_scan_with_report_parses() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest", "--report", "--scan", "trivy"]);
        assert_eq!(cli.scan, Some(scan::ScanTool::Trivy));
    }

    #[test]
    fn validate_scan_args_errors_when_custom_missing_scan_cmd() {
        let cli = Cli::parse_from(["imgchk", "nginx:latest", "--report", "--scan", "custom"]);
        assert!(validate_scan_args(&cli).is_err());
    }

    #[test]
    fn validate_scan_args_errors_when_scan_cmd_given_without_custom() {
        let cli = Cli::parse_from([
            "imgchk",
            "nginx:latest",
            "--report",
            "--scan",
            "trivy",
            "--scan-cmd",
            "foo {path}",
        ]);
        assert!(validate_scan_args(&cli).is_err());
    }

    #[test]
    fn validate_scan_args_ok_for_valid_custom_combination() {
        let cli = Cli::parse_from([
            "imgchk",
            "nginx:latest",
            "--report",
            "--scan",
            "custom",
            "--scan-cmd",
            "foo {path}",
        ]);
        assert!(validate_scan_args(&cli).is_ok());
    }
}
