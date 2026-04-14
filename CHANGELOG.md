# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1]

### Added
- Linux `aarch64-unknown-linux-gnu` release binary, built natively on `ubuntu-24.04-arm`.

### Changed
- Bumped workflow action pins to Node.js 24-compatible majors (`upload-artifact@v7`, `download-artifact@v8`, `dorny/paths-filter@v4`) ahead of GitHub's September 2026 Node.js 20 removal.
- `cargo nextest run` now passes `--no-tests=pass` so CI is green until test modules exist.

## [0.2.0]

### Added
- Release workflow: builds Linux x86_64 and macOS aarch64 binaries on `Cargo.toml` version bump or manual dispatch, publishes a GitHub Release with notes extracted from this file.
- CI workflow expanded with `paths-filter` change detection, `cargo fmt` / `cargo clippy` / `cargo nextest` / `cargo audit`, and cached linux + macOS release builds.
- CodeQL analysis for Rust and GitHub Actions on push, PR, and a weekly schedule.
- Dependabot auto-merge for patch and minor dependency PRs.
- Apache License 2.0.

### Changed
- Refreshed `Cargo.lock` to latest compatible versions. Bumps `rand` to 0.9.4, clearing RUSTSEC-2026-0097 on the `quinn-proto` path. Remaining audit warnings (`number_prefix` via `indicatif`, `rand` 0.8.5 via `termwiz`) require upstream releases.
