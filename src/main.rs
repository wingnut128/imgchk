mod extract;
mod image;
mod tree;
mod ui;

use std::path::PathBuf;

use clap::Parser;

/// Container image inspector and layer extraction tool.
#[derive(Parser)]
#[command(name = "imgchk", version, about)]
struct Cli {
    /// Image reference (e.g., nginx:latest, ghcr.io/org/app:v1.2) or path to a tarball
    image: String,

    /// Output directory for extracted files/layers
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Target platform (e.g., linux/amd64, linux/arm64)
    #[arg(long)]
    platform: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let image = if image::is_tarball(&cli.image) {
        image::load_tarball(std::path::Path::new(&cli.image))?
    } else {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(image::load_registry(
            &cli.image,
            cli.platform.as_deref(),
        ))?
    };

    if image.layers.is_empty() {
        anyhow::bail!("No layers found in image");
    }

    ui::run(image, cli.output)
}
