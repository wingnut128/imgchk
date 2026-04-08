# imgchk

A terminal UI tool for inspecting Docker and OCI container images. Browse layers, explore the filesystem tree, and extract files in multiple formats.

## Features

- **Registry & tarball loading** — pull images from Docker Hub, GHCR, or any OCI registry, or load from `docker save` tarballs
- **Layer browser** — navigate layers with metadata (size, digest, creation command)
- **File tree explorer** — browse each layer's filesystem with expand/collapse, selection, and cumulative view
- **Multiple export formats** — extract as tar.gz, tar, squashfs, or directory via [ocirender](https://crates.io/crates/ocirender)
- **Whiteout handling** — correctly merges `.wh.*` deletions and opaque whiteouts in cumulative view
- **Registry auth** — reads credentials from Docker's credential store or environment variables
- **Blob caching** — downloaded layers are cached locally to avoid redundant pulls

## Requirements

- **Rust 1.85+** (for building)
- **mksquashfs** (optional, only needed for squashfs export format)

### Installing mksquashfs

**macOS:**
```bash
brew install squashfs
```

**Debian/Ubuntu:**
```bash
sudo apt-get install squashfs-tools
```

**Fedora/RHEL:**
```bash
sudo dnf install squashfs-tools
```

**Arch Linux:**
```bash
sudo pacman -S squashfs-tools
```

If `mksquashfs` is not installed, all other export formats (tar.gz, tar, directory) work normally. The squashfs option will show an error in the status bar if the binary is not found.

## Installation

```bash
git clone https://github.com/wingnut128/imgchk.git
cd imgchk
cargo install --path .
```

Or build locally:

```bash
make build          # debug build
make release        # optimized release build
make hooks          # install pre-commit hook (fmt + clippy)
```

## Usage

```bash
# Inspect an image from Docker Hub
imgchk nginx:latest

# Inspect with a specific platform
imgchk --platform linux/arm64 alpine:3.19

# Inspect a local tarball (from docker save)
imgchk ./myimage.tar

# Set an output directory for extractions
imgchk -o /tmp/extracted ghcr.io/org/app:v1.2
```

## TUI Layout

```
┌──────────────┬─────────────────────────┐
│              │   File Tree             │
│  Layer List  │   [✓] ▾ usr/           │
│              │   [ ]   ▸ bin/         │
│  ▸ Layer 0   │   [✓]   passwd  1.2 KB │
│    Layer 1   │   [ ]   shadow   494 B │
│    Layer 2   │                         │
│              ├─────────────────────────┤
│              │   Details               │
│              │   Size: 4.2 MB Files: 8 │
│              │   $ apt-get install ... │
├──────────────┴─────────────────────────┤
│ nginx:latest │ linux/amd64 │ fmt:tar.gz│
└────────────────────────────────────────┘
```

### Keybindings

| Key | Action |
|-----|--------|
| `j`/`k`, Up/Down | Navigate within focused pane |
| `Tab` | Cycle focus (Layers -> Files -> Details) |
| `Enter` | Expand/collapse directory |
| `Space` | Select/deselect file or directory |
| `t` | Toggle cumulative vs single-layer view |
| `f` | Cycle export format (tar.gz, tar, squashfs, dir) |
| `o` | Set output directory |
| `e` | Extract (selected files or current layer) |
| `a` | Export all layers (merged) |
| `q` | Quit |

## Export Formats

| Format | Key | Description |
|--------|-----|-------------|
| **tar.gz** | default | Individual layer tarballs (gzipped) |
| **tar** | `f` | Merged filesystem as plain tar archive |
| **squashfs** | `f` | Merged filesystem as squashfs image (requires `mksquashfs`) |
| **dir** | `f` | Merged filesystem extracted to a directory |

Press `f` to cycle formats. The current format is shown in the status bar. Then use `e` (single layer) or `a` (all layers merged) to export.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `IMGCHK_REGISTRY_USER` | Registry username |
| `IMGCHK_REGISTRY_TOKEN` | Registry password or token |
| `IMGCHK_CACHE_DIR` | Override blob cache directory (default: `~/.cache/imgchk/blobs/`) |
| `IMGCHK_CACHE_MAX_MB` | Max cache size in MB (default: 10240) |

Authentication is resolved in order: environment variables, Docker credential store (`~/.docker/config.json`), then anonymous.

## Dependencies

- [oci-client](https://crates.io/crates/oci-client) — OCI registry client (ORAS/CNCF)
- [ocirender](https://crates.io/crates/ocirender) — OCI image conversion (squashfs, tar, directory)
- [ratatui](https://crates.io/crates/ratatui) — TUI framework
- [clap](https://crates.io/crates/clap) — CLI argument parsing
- [tokio](https://crates.io/crates/tokio) — async runtime

## License

Apache-2.0
