# `--report` non-interactive report mode

## Problem

imgchk is TUI-only today. There's no way to use it in CI or scripts — every
invocation launches an interactive terminal session. Image fetching and
analysis (layer metadata, file trees) already happens before the TUI starts;
none of that data is currently exposed outside the interactive view.

## Goal

Add a `--report` flag that fetches and analyzes the image exactly as today,
then prints a structured JSON report to stdout and exits, without launching
the TUI. This unlocks CI/scripting use cases (size checks, suspicious-file
scanning, layer auditing) on top of data the tool already computes.

## Non-goals (v1)

- No exit-code gating (`--fail-on-size`, `--fail-on-suspicious`, etc.) — the
  report is JSON only; callers gate however they want via `jq` + shell.
- No image signature verification. Signing checks require a registry call
  path imgchk doesn't have (cosign tag scheme or OCI Referrers API), a trust
  anchor (key file or Sigstore keyless/Rekor), and a new crypto dependency.
  That's a separate spec. This change reserves a `signature` field in the
  output, fixed at `null`, so the schema doesn't need to break when signing
  support lands.
- No full file-tree dump (paths, all files) in the report — only summary
  counts and the suspicious-file findings below. Full file listings can be a
  future `--report-files` addition if needed.

## CLI surface

New boolean flag on the existing `Cli` struct in `src/main.rs`:

```
--report   Print a JSON analysis report to stdout instead of launching the TUI
```

When `cli.report` is set, `main()`:
1. Resolves the image exactly as it does today (`TarballSource` or
   `RegistrySource`, same `--platform` handling).
2. Builds a `ReportImage` from the resulting `ImageInfo`.
3. Serializes it with `serde_json::to_string_pretty` and prints to stdout.
4. Returns `Ok(())` before any `ratatui` terminal setup.

Fetch errors behave as they do today (propagated via `anyhow::Result`,
non-zero exit, message on stderr). No new error states are introduced.

## Report shape

```json
{
  "source": "nginx:latest",
  "architecture": "amd64",
  "os": "linux",
  "total_size": 142312345,
  "signature": null,
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
        {"path": "/usr/bin/sudo", "reason": "setuid", "mode": 2479},
        {"path": "/etc/foo.pem", "reason": "secret_pattern", "mode": null}
      ]
    }
  ]
}
```

Field notes:
- `command` is the full, untruncated command (`command_format::clean_command`,
  not `truncate_command` — the latter is a TUI display concern).
- `file_count` and per-layer `size`/`digest`/`diff_id`/`created` come directly
  from the existing `LayerInfo`.
- `suspicious_files` is scoped to that layer's own `FileTree` (not the
  cumulative/merged view) — it reflects what that layer introduces.
- `signature` is always `null` in this change; reserved for a future spec.

## Suspicious-file detection

Implemented as a pure function over `&FileTree`, run per layer. Three rule
kinds, in this priority order (a file can match more than one rule — emit one
finding per matching rule):

| reason            | condition                                              | mode field |
|-------------------|---------------------------------------------------------|-----------|
| `setuid`          | regular file, `mode & 0o4000 != 0`                      | mode bits, octal value as decimal |
| `setgid`          | regular file, `mode & 0o2000 != 0`                       | mode bits |
| `world_writable`  | regular file, `mode & 0o002 != 0`                        | mode bits |
| `secret_pattern`  | filename matches a fixed pattern list (below)            | `null` (mode is irrelevant to the match) |

Secret pattern list (filename or extension match, case-sensitive, checked
against the final path segment):
`id_rsa`, `id_dsa`, `id_ecdsa`, `id_ed25519`, `*.pem`, `*.key`, `*.p12`,
`.env`

Directories and symlinks are skipped for all rules — these checks apply to
regular file entries only.

## Implementation shape

New module `src/report.rs`:
- `ReportImage`, `ReportLayer`, `SuspiciousFile` structs deriving
  `serde::Serialize`, matching the JSON shape above.
- `build_report(image: &ImageInfo) -> ReportImage` — pure transform, no I/O.
- `scan_suspicious(tree: &FileTree) -> Vec<SuspiciousFile>` — walks the tree,
  applies the three rules.

`src/main.rs` changes:
- Add `report: bool` field to `Cli` (`#[arg(long)]`).
- After loading `ImageInfo`, branch: if `cli.report`, call
  `report::build_report`, print JSON, return — before the existing
  `App`/terminal setup code.
- Add `mod report;` alongside the other module declarations.

## Testing

Unit tests in `src/report.rs`:
- `build_report` on a small synthetic `ImageInfo`/`LayerInfo` produces the
  expected JSON field names and values (serialize and check via
  `serde_json::Value` or snapshot string).
- `scan_suspicious` against a hand-built `FileTree`:
  - setuid/setgid/world-writable regular files are flagged with correct mode
  - a directory with the same mode bits is *not* flagged
  - each secret-pattern filename matches; a similar-but-non-matching name
    (e.g. `keys.txt`) does not
  - a file matching both a mode rule and a secret pattern produces two
    findings
- No existing tests should need changes — this is purely additive.
