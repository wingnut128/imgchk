# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.15](https://github.com/wingnut128/imgchk/compare/v0.4.14...v0.4.15) - 2026-08-12

### Other

- add release attestation and SBOM verification guide ([#134](https://github.com/wingnut128/imgchk/pull/134))

## [0.4.14](https://github.com/wingnut128/imgchk/compare/v0.4.13...v0.4.14) - 2026-08-12

### Fixed

- load docker save archives and clean up staged layer blobs ([#132](https://github.com/wingnut128/imgchk/pull/132))

### Other

- cache merged tree, reuse Tokio runtime; fix terminal cleanup and LRU atime ([#133](https://github.com/wingnut128/imgchk/pull/133))
- *(deps)* stop Dependabot proposing standalone oci-spec bumps ([#131](https://github.com/wingnut128/imgchk/pull/131))
- *(release)* attest build provenance and publish a CycloneDX SBOM ([#129](https://github.com/wingnut128/imgchk/pull/129))

## [0.4.13](https://github.com/wingnut128/imgchk/compare/v0.4.12...v0.4.13) - 2026-08-12

### Other

- *(deps)* bump taiki-e/install-action from 2.85.7 to 2.85.11 ([#126](https://github.com/wingnut128/imgchk/pull/126))
- (docs): Updates to Claude.md ([#128](https://github.com/wingnut128/imgchk/pull/128))
- *(deps)* bump github/codeql-action/analyze from 4.37.4 to 4.37.6 ([#125](https://github.com/wingnut128/imgchk/pull/125))
- *(deps)* bump clap in the cargo-minor-and-patch group ([#122](https://github.com/wingnut128/imgchk/pull/122))
- *(deps)* bump github/codeql-action/init from 4.37.4 to 4.37.6 ([#124](https://github.com/wingnut128/imgchk/pull/124))
- *(deps)* bump step-security/harden-runner from 2.20.0 to 2.20.1 ([#121](https://github.com/wingnut128/imgchk/pull/121))
- Add unit tests for image/registry.rs and extract/mod.rs ([#120](https://github.com/wingnut128/imgchk/pull/120))
- *(deps)* bump github/codeql-action/init from 4.37.3 to 4.37.4 ([#116](https://github.com/wingnut128/imgchk/pull/116))
- Add integration-test scaffold under tests/ ([#118](https://github.com/wingnut128/imgchk/pull/118))
- *(deps)* bump github/codeql-action/analyze from 4.37.3 to 4.37.4 ([#115](https://github.com/wingnut128/imgchk/pull/115))
- *(deps)* bump clap in the cargo-minor-and-patch group ([#113](https://github.com/wingnut128/imgchk/pull/113))
- *(deps)* bump taiki-e/install-action from 2.85.3 to 2.85.7 ([#114](https://github.com/wingnut128/imgchk/pull/114))

## [0.4.12](https://github.com/wingnut128/imgchk/compare/v0.4.11...v0.4.12) - 2026-07-29

### Other

- *(deps)* bump ocirender in the cargo-minor-and-patch group ([#109](https://github.com/wingnut128/imgchk/pull/109))
- *(deps)* bump step-security/paths-filter from 4.0.1 to 4.0.2 ([#111](https://github.com/wingnut128/imgchk/pull/111))
- *(deps)* bump taiki-e/install-action from 2.84.0 to 2.85.3 ([#107](https://github.com/wingnut128/imgchk/pull/107))
- *(deps)* bump github/codeql-action/analyze from 4.37.2 to 4.37.3 ([#110](https://github.com/wingnut128/imgchk/pull/110))
- *(deps)* bump github/codeql-action/init from 4.37.2 to 4.37.3 ([#108](https://github.com/wingnut128/imgchk/pull/108))
- *(deps)* bump actions/checkout from 7.0.0 to 7.0.1 ([#102](https://github.com/wingnut128/imgchk/pull/102))
- *(deps)* bump the cargo-minor-and-patch group with 5 updates ([#105](https://github.com/wingnut128/imgchk/pull/105))
- *(deps)* bump taiki-e/install-action from 2.83.2 to 2.84.0 ([#103](https://github.com/wingnut128/imgchk/pull/103))
- *(deps)* bump github/codeql-action/init from 4.36.3 to 4.37.2 ([#104](https://github.com/wingnut128/imgchk/pull/104))
- *(deps)* bump github/codeql-action/analyze from 4.36.3 to 4.37.2 ([#101](https://github.com/wingnut128/imgchk/pull/101))

## [0.4.11](https://github.com/wingnut128/imgchk/compare/v0.4.10...v0.4.11) - 2026-07-15

### Fixed

- pin codeql-action back to v4.36.3 to fix version-check failure ([#100](https://github.com/wingnut128/imgchk/pull/100))

### Other

- *(deps)* bump taiki-e/install-action from 2.82.10 to 2.83.2 ([#98](https://github.com/wingnut128/imgchk/pull/98))
- *(deps)* bump release-plz/action from 0.5.130 to 0.5.131 ([#97](https://github.com/wingnut128/imgchk/pull/97))
- *(deps)* bump github/codeql-action/analyze from 4.36.3 to 4.37.0 ([#96](https://github.com/wingnut128/imgchk/pull/96))
- *(deps)* bump github/codeql-action/init from 4.36.3 to 4.37.0 ([#95](https://github.com/wingnut128/imgchk/pull/95))

## [0.4.10](https://github.com/wingnut128/imgchk/compare/v0.4.9...v0.4.10) - 2026-07-10

### Fixed

- treat Trivy "Results": null as an empty scan, not a parse failure (ENG-115) ([#93](https://github.com/wingnut128/imgchk/pull/93))

## [0.4.9](https://github.com/wingnut128/imgchk/compare/v0.4.8...v0.4.9) - 2026-07-10

### Other

- authenticate dependabot auto-merge with dedicated DEPENDABOT_PAT (ENG-119) ([#92](https://github.com/wingnut128/imgchk/pull/92))
- *(deps)* bump github/codeql-action/analyze from 4.36.2 to 4.36.3 ([#82](https://github.com/wingnut128/imgchk/pull/82))
- *(deps)* bump github/codeql-action/init from 4.36.2 to 4.36.3 ([#81](https://github.com/wingnut128/imgchk/pull/81))
- *(deps)* bump indicatif in the cargo-minor-and-patch group ([#80](https://github.com/wingnut128/imgchk/pull/80))
- *(deps)* bump step-security/harden-runner from 2.19.4 to 2.20.0 ([#79](https://github.com/wingnut128/imgchk/pull/79))
- *(deps)* bump taiki-e/install-action from 2.82.7 to 2.82.10 ([#78](https://github.com/wingnut128/imgchk/pull/78))

## [0.4.8](https://github.com/wingnut128/imgchk/compare/v0.4.7...v0.4.8) - 2026-07-10

### Fixed

- sanitize terminal escapes in --scan and --dockerfile output (ENG-117) ([#90](https://github.com/wingnut128/imgchk/pull/90))

### Other

- exclude release-plz Release PRs from trusted auto-merge ([#88](https://github.com/wingnut128/imgchk/pull/88))

## [0.4.7](https://github.com/wingnut128/imgchk/compare/v0.4.6...v0.4.7) - 2026-07-10

### Added

- --dockerfile — extract & reconstruct build instructions ([#86](https://github.com/wingnut128/imgchk/pull/86))

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
