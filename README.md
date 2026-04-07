# imgchk

A terminal UI tool for inspecting Docker and OCI container images. Browse layers, explore the filesystem tree, and extract files interactively.

## Features

- **Multi-source loading** — load images from tarballs (`docker save`), the local Docker daemon, or remote registries
- **Layer browser** — navigate all image layers with metadata (size, digest, creation command)
- **File tree explorer** — browse the filesystem of each layer with expand/collapse navigation
- **Cumulative view** — toggle between single-layer and cumulative filesystem views with proper whiteout handling
- **File extraction** — select files with a checkbox picker and extract them to a local directory
- **Whiteout awareness** — correctly handles `.wh.*` deletions and opaque whiteouts when merging layers

## Installation

```bash
# From source
git clone <repo-url>
cd imgchk
make install

# Or just build locally
make build
```

Requires Go 1.26+.

## Usage

```bash
# Inspect a tarball (from docker save)
docker save nginx:latest -o nginx.tar
imgchk nginx.tar

# Inspect from Docker daemon
imgchk nginx:latest

# Inspect from remote registry
imgchk ghcr.io/owner/image:tag

# Extract files to a specific directory
imgchk -o /tmp/extracted nginx.tar
```

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-o` | `.` | Output directory for extracted files |

## TUI Layout

```
┌──────────────┬─────────────────────────┐
│              │   File Tree             │
│  Layer List  │   ├── usr/              │
│              │   │   ├── bin/          │
│  > Layer 0   │   │   │   └── [x] bash │
│    Layer 1   │   │   └── lib/          │
│    Layer 2   │   └── etc/              │
│              ├─────────────────────────┤
│              │   Details               │
│              │   Command: RUN apt-get… │
│              │   Digest: sha256:abc…   │
│              │   Size: 4.2 MB          │
├──────────────┴─────────────────────────┤
│ tab:pane ↑↓:nav space:select e:extract │
└────────────────────────────────────────┘
```

### Keybindings

| Key | Action |
|-----|--------|
| `↑` / `↓` / `j` / `k` | Navigate within the focused pane |
| `Tab` | Cycle focus between panes |
| `Enter` | Expand or collapse a directory |
| `Space` | Toggle file selection (directories select all children) |
| `t` | Toggle cumulative vs single-layer view |
| `e` | Extract selected files to the output directory |
| `q` / `Ctrl+C` | Quit |

## How It Works

1. The image is loaded and each layer's tar stream is parsed into an in-memory file tree
2. Layer metadata is correlated with the image config history (skipping empty/metadata-only layers)
3. The TUI displays three panes: layer list, file tree browser, and details panel
4. In cumulative mode, layers are merged bottom-up with OCI whiteout semantics applied
5. Extraction uses `mutate.Extract` for the flattened filesystem or reads directly from individual layer tars

## Project Structure

```
imgchk/
├── main.go                        # CLI entry point
├── internal/
│   ├── image/
│   │   ├── loader.go              # Image loading (tarball, daemon, registry)
│   │   ├── layer.go               # Layer/image metadata and analysis
│   │   └── filetree.go            # File tree construction and whiteout merging
│   ├── extract/
│   │   └── extract.go             # File extraction to disk
│   └── ui/
│       ├── app.go                 # Root TUI model and layout
│       ├── keys.go                # Key bindings
│       ├── styles.go              # Terminal styling
│       ├── layerlist.go           # Layer list pane
│       ├── filetree.go            # File tree browser pane
│       ├── details.go             # Details pane
│       └── statusbar.go           # Status bar
├── Makefile
├── go.mod
└── go.sum
```

## Dependencies

- [go-containerregistry](https://github.com/google/go-containerregistry) — Docker/OCI image handling
- [bubbletea](https://github.com/charmbracelet/bubbletea) — TUI framework
- [bubbles](https://github.com/charmbracelet/bubbles) — TUI components
- [lipgloss](https://github.com/charmbracelet/lipgloss) — Terminal styling and layout

## License

MIT
