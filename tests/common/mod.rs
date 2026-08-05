//! Fixture builders and process helpers for black-box CLI tests.
//!
//! `imgchk` is a binary-only crate (no `src/lib.rs`), so integration tests
//! can't call its internals directly — everything here drives the compiled
//! binary as a subprocess against tarball fixtures built to match the two
//! shapes `TarballSource` understands (see `src/image/tarball.rs`):
//! a `docker save`-style archive (manifest.json + config.json + layer
//! tars) and a bare single-layer tar/tar.gz.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// A `Command` for the binary built from this package, ready for `.arg(..)`.
pub fn imgchk() -> Command {
    Command::new(env!("CARGO_BIN_EXE_imgchk"))
}

fn unique_temp_path(suffix: &str) -> PathBuf {
    // Tests run in parallel and share this process's PID, so a counter
    // (rather than a timestamp) guarantees distinct fixture paths.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    path.push(format!("imgchk-cli-test-{pid}-{seq}{suffix}"));
    path
}

pub fn write_temp(bytes: &[u8], suffix: &str) -> PathBuf {
    let path = unique_temp_path(suffix);
    std::fs::write(&path, bytes).unwrap();
    path
}

pub struct FixtureFile {
    pub path: &'static str,
    pub contents: &'static [u8],
    pub mode: u32,
}

pub fn tar_bytes(files: &[FixtureFile]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for f in files {
        let mut header = tar::Header::new_gnu();
        header.set_path(f.path).unwrap();
        header.set_size(f.contents.len() as u64);
        header.set_mode(f.mode);
        header.set_cksum();
        builder.append(&header, f.contents).unwrap();
    }
    builder.into_inner().unwrap()
}

pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes).unwrap();
    enc.finish().unwrap()
}

/// A gzip-compressed bare layer tarball — no `manifest.json` — the shape
/// `load_single_layer` handles. Its `blob_path` in the loaded `LayerInfo`
/// is the real file on disk, so this is also what `--scan` needs (unlike
/// a docker-archive fixture, whose per-layer `blob_path` is a fake
/// in-archive path — see the note on `docker_archive_tarball`).
pub fn single_layer_tarball(files: &[FixtureFile]) -> PathBuf {
    write_temp(&gzip(&tar_bytes(files)), ".tar.gz")
}

fn append_file(builder: &mut tar::Builder<Vec<u8>>, path: &str, data: &[u8]) {
    let mut h = tar::Header::new_gnu();
    h.set_path(path).unwrap();
    h.set_size(data.len() as u64);
    h.set_mode(0o644);
    h.set_cksum();
    builder.append(&h, data).unwrap();
}

pub struct HistoryEntry {
    pub created_by: &'static str,
    pub empty_layer: bool,
}

pub struct DockerArchiveSpec<'a> {
    pub architecture: &'a str,
    pub os: &'a str,
    pub history: &'a [HistoryEntry],
    pub layer_files: &'a [FixtureFile],
}

/// Build a `docker save`-shaped archive (manifest.json + config.json +
/// layer0/layer.tar), the shape `load_docker_archive` expects.
///
/// Note: the resulting `LayerInfo.blob_path` is the in-archive path
/// (`"layer0/layer.tar"`), not a real file on disk — matching
/// `load_docker_archive`'s current behavior — so fixtures built this way
/// can't be used for `--scan`, which reads layers back from `blob_path`.
/// Use `single_layer_tarball` for scan tests.
pub fn docker_archive_tarball(spec: &DockerArchiveSpec) -> PathBuf {
    let layer_tar = tar_bytes(spec.layer_files);

    let history_json: Vec<serde_json::Value> = spec
        .history
        .iter()
        .map(|h| {
            serde_json::json!({
                "created_by": h.created_by,
                "created": "2026-01-01T00:00:00Z",
                "empty_layer": h.empty_layer,
            })
        })
        .collect();

    let config = serde_json::json!({
        "architecture": spec.architecture,
        "os": spec.os,
        "history": history_json,
        "rootfs": {"type": "layers", "diff_ids": ["sha256:abc"]},
    });
    let manifest = serde_json::json!([{
        "Config": "config.json",
        "RepoTags": [],
        "Layers": ["layer0/layer.tar"],
    }]);

    let mut builder = tar::Builder::new(Vec::new());
    append_file(
        &mut builder,
        "manifest.json",
        &serde_json::to_vec(&manifest).unwrap(),
    );
    append_file(
        &mut builder,
        "config.json",
        &serde_json::to_vec(&config).unwrap(),
    );
    append_file(&mut builder, "layer0/layer.tar", &layer_tar);

    write_temp(&builder.into_inner().unwrap(), ".tar")
}

/// Same manifest shape as `docker_archive_tarball` but with zero layers —
/// exercises `main.rs`'s "No layers found in image" bail path.
pub fn docker_archive_tarball_no_layers() -> PathBuf {
    let config = serde_json::json!({"architecture": "amd64", "os": "linux"});
    let empty_layers: Vec<String> = Vec::new();
    let manifest = serde_json::json!([{
        "Config": "config.json",
        "RepoTags": [],
        "Layers": empty_layers,
    }]);

    let mut builder = tar::Builder::new(Vec::new());
    append_file(
        &mut builder,
        "manifest.json",
        &serde_json::to_vec(&manifest).unwrap(),
    );
    append_file(
        &mut builder,
        "config.json",
        &serde_json::to_vec(&config).unwrap(),
    );

    write_temp(&builder.into_inner().unwrap(), ".tar")
}
