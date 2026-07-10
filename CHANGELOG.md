# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.6](https://github.com/wingnut128/imgchk/compare/v0.4.5...v0.4.6) - 2026-07-10

### Added

- friendlier --scan output (human summary + normalized scan.summary) ([#83](https://github.com/wingnut128/imgchk/pull/83))

### Other

- fix flaky write_temp temp-file name collision (ENG-116) ([#84](https://github.com/wingnut128/imgchk/pull/84))

## [0.4.5](https://github.com/wingnut128/imgchk/compare/v0.4.4...v0.4.5) - 2026-07-02

### Other

- Add --scan flag: plug in external vulnerability scanners ([#76](https://github.com/wingnut128/imgchk/pull/76))

## [0.4.4](https://github.com/wingnut128/imgchk/compare/v0.4.3...v0.4.4) - 2026-07-01

### Other

- Add severity to suspicious-file findings + jq recipes ([#74](https://github.com/wingnut128/imgchk/pull/74))

## [0.4.3](https://github.com/wingnut128/imgchk/compare/v0.4.2...v0.4.3) - 2026-07-01

### Other

- authenticate auto-merge with RELEASE_PLZ_TOKEN, not GITHUB_TOKEN ([#72](https://github.com/wingnut128/imgchk/pull/72))
- Exclude device/FIFO nodes from suspicious-file scan ([#71](https://github.com/wingnut128/imgchk/pull/71))
- Add --report non-interactive JSON mode ([#70](https://github.com/wingnut128/imgchk/pull/70))

## [0.4.2](https://github.com/wingnut128/imgchk/compare/v0.4.1...v0.4.2) - 2026-06-30

### Other

- *(deps)* bump actions/checkout from 6.0.3 to 7.0.0 ([#60](https://github.com/wingnut128/imgchk/pull/60))
- *(deps)* bump actions/cache from 5.0.5 to 6.1.0 ([#61](https://github.com/wingnut128/imgchk/pull/61))
- *(deps)* bump the cargo-minor-and-patch group across 1 directory with 3 updates ([#67](https://github.com/wingnut128/imgchk/pull/67))
- *(deps)* bump taiki-e/install-action from 2.81.11 to 2.82.7 ([#63](https://github.com/wingnut128/imgchk/pull/63))
- *(deps)* bump quinn-proto to 0.11.15 for RUSTSEC-2026-0185 ([#64](https://github.com/wingnut128/imgchk/pull/64))
- enable release-plz auto-bump via git_only ([#65](https://github.com/wingnut128/imgchk/pull/65))
- *(deps)* bump taiki-e/install-action from 2.81.10 to 2.81.11 ([#58](https://github.com/wingnut128/imgchk/pull/58))
- *(deps)* bump actions/checkout from 6.0.2 to 6.0.3 ([#59](https://github.com/wingnut128/imgchk/pull/59))

## [0.4.1] - 2026-06-15

### Tests
- Added a content-level assertion for tarball loading: a `docker save` archive's layer file tree is verified to contain `/usr/bin/hello` with the expected path, size, and mode, complementing the existing count-only checks.

## [0.4.0] - 2026-06-15

### Added
- After extracting files, the status bar now shows the produced archive path, so outputs are easy to locate without digging through the tmpdir.

### Changed
- `extract_with` now returns the output paths it produced, surfacing them to the caller (and the TUI) instead of discarding them.
- Dependency bumps: `oci-client` 0.16.1 → 0.17.0, `ratatui` 0.30.0 → 0.30.1, `tar` 0.4.45 → 0.4.46, `serde_json` 1.0.149 → 1.0.150, `tokio` 1.52.1 → 1.52.3.

### Security
- Cleared RUSTSEC-2026-0173 (`proc-macro-error2` unmaintained) — the `oci-client` 0.17 bump pulls in transitive `getset` 0.1.6 → 0.1.7, which drops `proc-macro-error2` entirely.

### CI
- Dependabot auto-merge now uses the official `dependabot/fetch-metadata@v3.1.0` instead of StepSecurity's hosted fork, which had started failing with "Subscription is not valid" and blocked auto-merge of every Dependabot PR.
- Action pin bumps: `taiki-e/install-action` 2.75.29 → 2.81.10, `github/codeql-action` 4.35.3 → 4.36.2, `step-security/harden-runner` 2.19.1 → 2.19.4, `actions/dependency-review-action` 4.9.0 → 5.0.0, `release-plz/action` 0.5.128 → 0.5.130.

### Docs
- Documented the release-plz `publish=false` manual-bump release workflow.

## [0.3.1] - 2026-05-04

### Fixed
- `imgchk <path>` now sniffs gzip magic bytes at the input boundary and falls back to single-layer inspection when the file has no `manifest.json`. Previously, pointing imgchk at one of its own extracted `layer-N.tar.gz` outputs failed with a tar-crate `numeric field did not have utf-8 text` cksum error. As a side benefit, gzipped Docker archives (`docker save … | gzip > out.tgz`) now also work.

## [0.3.0] - 2026-05-04

### Changed
- Decomposed `src/extract.rs` into `path_safety` / `selector::FileSelector` / `writer::OutputWriter` modules. Orchestration now flows through `extract_with(layer, &dyn FileSelector, Box<dyn OutputWriter>)`; `DirWriter`, `TarWriter`, and `TarGzWriter` each ship with round-trip smoke tests.
- Decomposed `src/image.rs` into `ImageSource` / `CredentialResolver` / `BlobStore`; folded `cache.rs` into `image/blobs.rs` as private helpers.
- Decomposed the flat `App` struct into `NavState` / `OutputState` / `ModalState`.
- Split `ui.rs` into `input` / `update` / `view` modules.
- Extracted a dedicated `Selection` module and moved tree-navigation methods onto it.

### Security
- `path_safety::safe_path` is now a named, public predicate that returns a validated `SafePath { absolute, relative }`. It drops `..` / `.` components, normalizes backslash separators, and rejects empty inputs, root-only paths, and Windows drive letters. 14 unit tests exercise the corners.

### CI
- New `pin-check` workflow enforces SHA-pinning of third-party actions on PRs.
- Dependabot now groups cargo minor + patch updates into a single PR.
- PRs that touch no code report success on the gating job instead of stalling.
- Action pin bumps: `step-security/harden-runner` 2.19.0 → 2.19.1, `github/codeql-action` 4.35.2 → 4.35.3, `taiki-e/install-action` 2.75.19 → 2.75.29.

### Tooling
- Adopted [release-plz](https://release-plz.dev) for version/CHANGELOG automation. Releases are now driven by conventional-commit messages on `main`.

## [0.2.2]

### Changed
- Migrated to Rust 2024 edition (requires rustc 1.85+).
- Bumped dependencies: `ocirender` 0.2.0 → 0.2.1, `indicatif` 0.17 → 0.18, `clap` 4.6.0 → 4.6.1, `tokio` 1.51 → 1.52, `crossterm` 0.28 → 0.29.

### Security
- Bumped `rustls-webpki` 0.103.12 → 0.103.13 for RUSTSEC-2026-0104 (reachable panic in CRL parsing).
- Ignored RUSTSEC-2026-0097 (unreachable `rand` 0.8 in the `sqlx-postgres` transitive chain); the vulnerable pattern isn't used.

### CI
- Pinned `taiki-e/install-action` to a release SHA; bumped `actions/upload-artifact` v7, `actions/checkout` v6, `github/codeql-action` v4, `ossf/scorecard-action` v2.4.3.
- `cargo-audit` wired into CI; StepSecurity hardening applied across workflows.
- Dependabot auto-merge now uses `GITHUB_TOKEN`.

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
