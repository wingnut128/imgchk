//! Black-box tests that run the compiled `imgchk` binary end to end against
//! tarball fixtures, covering the `--report` / `--dockerfile` / `--scan`
//! stdout-producing modes that never launch the TUI. These are deliberately
//! offline: tarball fixtures avoid any registry dependency, and the
//! `--report`/`--dockerfile` combination checks below fail during CLI
//! validation, before an image is ever loaded.

mod common;

use common::{
    DockerArchiveSpec, FixtureFile, HistoryEntry, docker_archive_tarball,
    docker_archive_tarball_no_layers, imgchk, single_layer_tarball,
};
use std::process::Output;

fn run(args: &[&str]) -> Output {
    imgchk().args(args).output().expect("failed to run imgchk")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn report_on_single_layer_tarball_is_valid_json() {
    let path = single_layer_tarball(&[FixtureFile {
        path: "usr/bin/hello",
        contents: b"hi\n",
        mode: 0o755,
    }]);

    let output = run(&[path.to_str().unwrap(), "--report"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let json: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("stdout should be valid JSON");

    assert!(json["signature"].is_null());
    assert!(json["scan"].is_null());
    assert_eq!(json["history"].as_array().unwrap().len(), 0);
    assert!(
        json["dockerfile"]
            .as_str()
            .unwrap()
            .contains("No build history")
    );

    let layers = json["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0]["file_count"], 1);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn report_on_docker_archive_flags_suspicious_files() {
    let path = docker_archive_tarball(&DockerArchiveSpec {
        architecture: "amd64",
        os: "linux",
        history: &[
            HistoryEntry {
                created_by: "/bin/sh -c #(nop)  ENV PATH=/usr/local/bin",
                empty_layer: true,
            },
            HistoryEntry {
                created_by: "/bin/sh -c apt-get update",
                empty_layer: false,
            },
        ],
        layer_files: &[
            FixtureFile {
                path: "usr/bin/app",
                contents: b"binary\n",
                // setuid + world-writable
                mode: 0o4777,
            },
            FixtureFile {
                path: "root/.ssh/id_rsa",
                contents: b"-----BEGIN PRIVATE KEY-----\n",
                mode: 0o600,
            },
        ],
    });

    let output = run(&[path.to_str().unwrap(), "--report"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let json: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("stdout should be valid JSON");

    assert_eq!(json["architecture"], "amd64");
    assert_eq!(json["os"], "linux");
    assert_eq!(json["history"].as_array().unwrap().len(), 2);

    let suspicious = json["layers"][0]["suspicious_files"].as_array().unwrap();
    let reasons: Vec<&str> = suspicious
        .iter()
        .map(|f| f["reason"].as_str().unwrap())
        .collect();
    assert!(reasons.contains(&"setuid"), "reasons: {reasons:?}");
    assert!(reasons.contains(&"world_writable"), "reasons: {reasons:?}");
    assert!(reasons.contains(&"secret_pattern"), "reasons: {reasons:?}");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn dockerfile_reconstructed_mode_prints_annotated_instructions() {
    let path = docker_archive_tarball(&DockerArchiveSpec {
        architecture: "amd64",
        os: "linux",
        history: &[
            HistoryEntry {
                created_by: "/bin/sh -c #(nop)  ENV PATH=/usr/local/bin",
                empty_layer: true,
            },
            HistoryEntry {
                created_by: "/bin/sh -c apt-get update",
                empty_layer: false,
            },
        ],
        layer_files: &[FixtureFile {
            path: "usr/bin/app",
            contents: b"binary\n",
            mode: 0o755,
        }],
    });

    let output = run(&[path.to_str().unwrap(), "--dockerfile"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let text = stdout(&output);
    assert!(text.contains("Reconstructed by imgchk"));
    assert!(text.contains("ENV PATH=/usr/local/bin"));
    assert!(text.contains("RUN apt-get update"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn dockerfile_raw_mode_prints_verbatim_history() {
    let path = docker_archive_tarball(&DockerArchiveSpec {
        architecture: "amd64",
        os: "linux",
        history: &[
            HistoryEntry {
                created_by: "/bin/sh -c #(nop)  ENV PATH=/usr/local/bin",
                empty_layer: true,
            },
            HistoryEntry {
                created_by: "/bin/sh -c apt-get update",
                empty_layer: false,
            },
        ],
        layer_files: &[FixtureFile {
            path: "usr/bin/app",
            contents: b"binary\n",
            mode: 0o755,
        }],
    });

    let output = run(&[path.to_str().unwrap(), "--dockerfile=raw"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output).trim(),
        "/bin/sh -c #(nop)  ENV PATH=/usr/local/bin\n/bin/sh -c apt-get update"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn empty_image_bails_with_no_layers_error() {
    let path = docker_archive_tarball_no_layers();

    let output = run(&[path.to_str().unwrap(), "--report"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("No layers found in image"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn no_image_arg_prints_help_and_exits_success() {
    let output = run(&[]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("EXAMPLES"));
}

#[test]
fn malformed_image_reference_fails_offline_without_hitting_a_registry() {
    // Not a tarball path and not a well-formed image reference — this must
    // fail during `Reference` parsing, never attempting a network call.
    let output = run(&["not a valid reference!!", "--report"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("invalid image reference"));
}

#[test]
fn dockerfile_and_scan_without_report_is_rejected_before_loading_image() {
    // Validation runs before the image is ever loaded, so this is a fast,
    // offline check even though "some-image" isn't resolvable.
    let output = run(&["some-image", "--dockerfile", "--scan", "trivy"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("cannot be combined without --report"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn scan_custom_without_scan_cmd_is_rejected_before_loading_image() {
    let output = run(&["some-image", "--report", "--scan", "custom"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("--scan=custom requires --scan-cmd"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn report_with_custom_scan_embeds_raw_output_and_null_summary() {
    let path = single_layer_tarball(&[FixtureFile {
        path: "usr/bin/hello",
        contents: b"hi\n",
        mode: 0o755,
    }]);

    let output = run(&[
        path.to_str().unwrap(),
        "--report",
        "--scan",
        "custom",
        "--scan-cmd",
        "echo scan-ran-ok",
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let json: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("stdout should be valid JSON");

    let scan = &json["scan"];
    assert_eq!(scan["tool"], "custom");
    assert!(scan["error"].is_null(), "scan: {scan}");
    assert!(scan["summary"].is_null(), "custom tool never normalizes");
    assert!(
        scan["output"]
            .as_str()
            .is_some_and(|s| s.contains("scan-ran-ok")),
        "scan.output: {}",
        scan["output"]
    );

    let _ = std::fs::remove_file(&path);
}
