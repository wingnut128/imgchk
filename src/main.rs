mod action;
mod command_format;
mod extract;
mod image;
mod report;
mod scan;
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

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
}
