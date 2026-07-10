# imgchk

A terminal UI tool for inspecting Docker and OCI container images. Browse layers, explore the filesystem tree, and extract files in multiple formats.

## Features

- **Registry & tarball loading** — pull images from Docker Hub, GHCR, or any OCI registry, or load from `docker save` tarballs
- **Non-interactive report mode** — `--report` prints a JSON analysis (layer metadata + suspicious-file findings) to stdout for CI/scripting use
- **Pluggable vulnerability scanning** — `--scan trivy`/`--scan grype`/`--scan custom` runs an external scanner against the merged image filesystem; standalone `--scan` prints a human-readable summary, and `--report --scan` embeds the raw output plus a normalized `scan.summary` in the JSON
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
just build          # debug build
just release        # optimized release build
just hooks          # install pre-commit hook (fmt + clippy)
```

Run `just` (or `just --list`) to see all available recipes.

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

# Print a JSON report instead of launching the TUI
imgchk nginx:latest --report
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

## Report Mode

`imgchk <image-ref> --report` fetches and analyzes the image exactly as the TUI does, then prints a JSON report to stdout and exits — no interactive session is launched. Useful for CI checks and scripting.

```json
{
  "source": "nginx:latest",
  "architecture": "amd64",
  "os": "linux",
  "total_size": 142312345,
  "signature": null,
  "scan": null,
  "layers": [
    {
      "index": 0,
      "digest": "sha256:...",
      "diff_id": "sha256:...",
      "size": 12345,
      "command": "RUN apt-get update && apt-get install -y curl",
      "created": "2026-01-01T00:00:00Z",
      "file_count": 482,
      "suspicious_files": [
        {"path": "/usr/bin/sudo", "reason": "setuid", "severity": "info", "mode": 2479},
        {"path": "/etc/foo.pem", "reason": "secret_pattern", "severity": "warning", "mode": null}
      ]
    }
  ]
}
```

Each layer's `suspicious_files` lists regular files (directories, symlinks, and device/FIFO nodes like `/dev/null` are never flagged) matching one or more of: `setuid`, `setgid`, `world_writable` (Unix mode bits), or `secret_pattern` (filenames like `id_rsa`, `*.pem`, `*.key`, `*.p12`, `.env`). A file matching multiple rules gets one entry per rule. Every finding carries a `severity`:

- `info` — `setuid`/`setgid`. Expected on any base-distro rootfs (`passwd`, `su`, `mount`, `chsh`, ...) — flagged for visibility, not itself an anomaly.
- `warning` — `world_writable`/`secret_pattern`. Actually unusual; worth a look.

The `signature` field is reserved for future image-signing verification and is always `null` today. Report mode does not set a non-zero exit code based on findings — pipe to `jq` and gate however your CI needs.

### Vulnerability scanning

`--scan <trivy|grype|custom>` extracts the image's merged filesystem to a temporary directory and runs an external scanner against it. It works in two modes:

- **Standalone** (`imgchk nginx:latest --scan trivy`, no `--report`): skips the TUI and prints a human-readable summary to stdout — a `<tool> · <image>` header, a counts line across all five severities (CRITICAL/HIGH/MEDIUM/LOW/UNKNOWN) plus a total, a table of Critical and High findings (SEVERITY/CVE/PACKAGE/INSTALLED/FIXED), and a footer summarizing any Medium/Low/Unknown counts with a pointer to `--report` for the full JSON.
- **Report** (`imgchk nginx:latest --report --scan trivy`): embeds the result in the JSON report, including both the raw scanner output and a normalized summary:

```json
"scan": {
  "tool": "trivy",
  "command": "trivy rootfs --format json /tmp/imgchk-scan-xyz",
  "exit_code": 0,
  "summary": {
    "counts": {"critical": 0, "high": 2, "medium": 5, "low": 3, "unknown": 0},
    "total": 10,
    "vulnerabilities": [
      {"id": "CVE-2026-40200", "package": "musl", "installed_version": "1.2.4_git20230717-r5", "fixed_version": "1.2.4_git20230717-r6", "severity": "high"}
    ]
  },
  "output": { "...": "trivy's own JSON output, embedded as-is" },
  "error": null
}
```

`trivy` and `grype` are built-in presets (`trivy rootfs --format json {path}` and `grype dir:{path} -o json` respectively). For any other scanner, use `--scan custom --scan-cmd '<template>'` with `{path}` as a placeholder for the extracted directory — for example:

```bash
imgchk nginx:latest --report --scan custom --scan-cmd 'mytool scan {path} --format json'
```

`output` holds the scanner's own JSON if its stdout parses as JSON, or a raw string otherwise. `error` is `null` on a normal run — a scanner exiting non-zero because it *found* vulnerabilities is not an imgchk-level error — and is only set when imgchk couldn't run the command at all (binary not found, failed to spawn). A scan failure never blocks the rest of the report; `layers`/`suspicious_files` always print.

`summary` is a normalized view derived from `output` — `counts` and `total` tally findings by severity, and `vulnerabilities` lists every finding as `{id, package, installed_version, fixed_version, severity}` (`fixed_version` is `null` when the scanner reports no fix yet). Normalization currently understands **Trivy and Grype output only**; for `--scan custom` (or any scanner output imgchk can't parse), `summary` is `null` and the raw `output` is still retained — pipe it through `jq` yourself.

**Security note:** imgchk shells out to whatever `trivy`/`grype` binary is on `PATH` — it does not pin a version or verify checksums. Run a known-good scanner version and verify its checksum before relying on it; Trivy had a supply-chain compromise in early 2026 (malicious `v0.69.4`–`v0.69.6` releases). The trust boundary here is your installed scanner binary, not imgchk.

### jq recipes

```bash
# Only the findings worth acting on (skip expected setuid/setgid noise)
imgchk nginx:latest --report | jq '.layers[].suspicious_files[] | select(.severity == "warning")'

# Fail CI if any warning-severity finding exists
imgchk nginx:latest --report | jq -e '[.layers[].suspicious_files[] | select(.severity == "warning")] | length == 0' > /dev/null

# Every suspicious file across all layers, flattened, with its layer index
imgchk nginx:latest --report | jq '[.layers[] | .index as $i | .suspicious_files[] | {layer: $i, path, reason, severity}]'

# Total image size in human-readable form
imgchk nginx:latest --report | jq -r '.total_size | tostring + " bytes"'

# Layers over 50MB, sorted largest first
imgchk nginx:latest --report | jq '[.layers[] | select(.size > 50000000)] | sort_by(-.size) | map({index, size, command})'

# Just the counts: how many suspicious files per layer
imgchk nginx:latest --report | jq '.layers[] | {index, suspicious_count: (.suspicious_files | length)}'

# Pull out scan findings, guarding against a failed/missing scanner
imgchk nginx:latest --report --scan trivy | jq 'if .scan.error then error(.scan.error) else .scan.output end'

# Severity counts from a scan
imgchk nginx:latest --report --scan trivy | jq '.scan.summary.counts'
```

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
