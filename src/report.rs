use serde::Serialize;

use crate::command_format::clean_command;
use crate::image::ImageInfo;
use crate::tree::{FileNode, FileTree};

#[derive(Serialize)]
pub struct ReportImage {
    pub source: String,
    pub architecture: String,
    pub os: String,
    pub total_size: u64,
    pub signature: Option<()>,
    pub layers: Vec<ReportLayer>,
}

#[derive(Serialize)]
pub struct ReportLayer {
    pub index: usize,
    pub digest: String,
    pub diff_id: String,
    pub size: u64,
    pub command: String,
    pub created: String,
    pub file_count: usize,
    pub suspicious_files: Vec<SuspiciousFile>,
}

#[derive(Serialize)]
pub struct SuspiciousFile {
    pub path: String,
    pub reason: &'static str,
    pub mode: Option<u32>,
}

pub fn build_report(image: &ImageInfo) -> ReportImage {
    ReportImage {
        source: image.source.clone(),
        architecture: image.architecture.clone(),
        os: image.os.clone(),
        total_size: image.total_size,
        signature: None,
        layers: image
            .layers
            .iter()
            .map(|layer| ReportLayer {
                index: layer.index,
                digest: layer.digest.clone(),
                diff_id: layer.diff_id.clone(),
                size: layer.size,
                command: clean_command(&layer.command),
                created: layer.created.clone(),
                file_count: layer.file_tree.file_count,
                suspicious_files: scan_suspicious(&layer.file_tree),
            })
            .collect(),
    }
}

const SECRET_EXACT_NAMES: &[&str] = &["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519", ".env"];
const SECRET_EXTENSIONS: &[&str] = &[".pem", ".key", ".p12"];

pub fn scan_suspicious(tree: &FileTree) -> Vec<SuspiciousFile> {
    let mut findings = Vec::new();
    walk(&tree.root, &mut findings);
    findings
}

// `node.mode` is the raw tar mode: only the low permission/special bits
// (setuid/setgid/world-writable) are meaningful here, regardless of whether
// type bits (S_IFMT) happen to be present.
fn walk(node: &FileNode, findings: &mut Vec<SuspiciousFile>) {
    if node.is_dir {
        for child in node.children.values() {
            walk(child, findings);
        }
        return;
    }

    // Symlinks are never flagged as suspicious — rules apply to regular files only
    if node.link_target.is_some() {
        return;
    }

    // Device/FIFO nodes (e.g. /dev/null) carry permission bits governed by
    // device semantics, not content access — a conventional 0o666 on
    // /dev/null isn't a security signal the way it would be on a regular file.
    if node.is_special {
        return;
    }

    if node.mode & 0o4000 != 0 {
        findings.push(SuspiciousFile {
            path: node.path.clone(),
            reason: "setuid",
            mode: Some(node.mode),
        });
    }
    if node.mode & 0o2000 != 0 {
        findings.push(SuspiciousFile {
            path: node.path.clone(),
            reason: "setgid",
            mode: Some(node.mode),
        });
    }
    if node.mode & 0o002 != 0 {
        findings.push(SuspiciousFile {
            path: node.path.clone(),
            reason: "world_writable",
            mode: Some(node.mode),
        });
    }
    if is_secret_pattern(&node.name) {
        findings.push(SuspiciousFile {
            path: node.path.clone(),
            reason: "secret_pattern",
            mode: None,
        });
    }
}

fn is_secret_pattern(name: &str) -> bool {
    SECRET_EXACT_NAMES.contains(&name) || SECRET_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::LayerInfo;
    use crate::tree::FileNode;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn empty_layer(index: usize, command: &str) -> LayerInfo {
        LayerInfo {
            index,
            digest: format!("sha256:digest{index}"),
            diff_id: format!("sha256:diffid{index}"),
            size: 1000 + index as u64,
            command: command.to_string(),
            created: "2026-01-01T00:00:00Z".to_string(),
            file_tree: FileTree::new(),
            blob_path: PathBuf::from("/tmp/blob"),
            media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
        }
    }

    #[test]
    fn build_report_maps_image_and_layer_fields() {
        let image = ImageInfo {
            layers: vec![empty_layer(0, "RUN   apt-get update")],
            total_size: 1000,
            architecture: "amd64".to_string(),
            os: "linux".to_string(),
            source: "nginx:latest".to_string(),
        };

        let report = build_report(&image);

        assert_eq!(report.source, "nginx:latest");
        assert_eq!(report.architecture, "amd64");
        assert_eq!(report.os, "linux");
        assert_eq!(report.total_size, 1000);
        assert!(report.signature.is_none());
        assert_eq!(report.layers.len(), 1);
        assert_eq!(report.layers[0].index, 0);
        assert_eq!(report.layers[0].digest, "sha256:digest0");
        assert_eq!(report.layers[0].command, "RUN apt-get update");
        assert_eq!(report.layers[0].file_count, 0);
    }

    #[test]
    fn build_report_serializes_signature_as_null() {
        let image = ImageInfo {
            layers: vec![],
            total_size: 0,
            architecture: "amd64".to_string(),
            os: "linux".to_string(),
            source: "alpine:3.19".to_string(),
        };

        let json = serde_json::to_string(&build_report(&image)).unwrap();
        assert!(json.contains("\"signature\":null"));
    }

    // Test helpers for scan_suspicious tests
    fn file_node(path: &str, mode: u32) -> FileNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        FileNode {
            name,
            path: path.to_string(),
            size: 100,
            mode,
            is_dir: false,
            is_whiteout: false,
            is_opaque: false,
            is_special: false,
            link_target: None,
            children: BTreeMap::new(),
        }
    }

    fn dir_node(path: &str, mode: u32) -> FileNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        FileNode {
            name,
            path: path.to_string(),
            size: 0,
            mode,
            is_dir: true,
            is_whiteout: false,
            is_opaque: false,
            is_special: false,
            link_target: None,
            children: BTreeMap::new(),
        }
    }

    fn symlink_node(path: &str, target: &str) -> FileNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        FileNode {
            name,
            path: path.to_string(),
            size: 0,
            mode: 0o120777, // symlink mode
            is_dir: false,
            is_whiteout: false,
            is_opaque: false,
            is_special: false,
            link_target: Some(target.to_string()),
            children: BTreeMap::new(),
        }
    }

    fn device_node(path: &str, mode: u32) -> FileNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        FileNode {
            name,
            path: path.to_string(),
            size: 0,
            mode,
            is_dir: false,
            is_whiteout: false,
            is_opaque: false,
            is_special: true,
            link_target: None,
            children: BTreeMap::new(),
        }
    }

    #[test]
    fn scan_suspicious_does_not_flag_world_writable_device_node() {
        // /dev/null, /dev/zero, /dev/random, /dev/urandom are conventionally
        // 0o666 (world-writable) — that's expected device-file permission,
        // not a security signal. Regression test for a false positive seen
        // on real apko-built images.
        let mut tree = FileTree::new();
        tree.insert_node("/dev/null", device_node("/dev/null", 0o666));

        let findings = scan_suspicious(&tree);

        assert!(findings.is_empty());
    }

    #[test]
    fn scan_suspicious_flags_setuid_file() {
        let mut tree = FileTree::new();
        tree.insert_node("/usr/bin/sudo", file_node("/usr/bin/sudo", 0o104755));

        let findings = scan_suspicious(&tree);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].path, "/usr/bin/sudo");
        assert_eq!(findings[0].reason, "setuid");
        assert_eq!(findings[0].mode, Some(0o104755));
    }

    #[test]
    fn scan_suspicious_flags_setgid_file() {
        let mut tree = FileTree::new();
        tree.insert_node("/usr/bin/wall", file_node("/usr/bin/wall", 0o102755));

        let findings = scan_suspicious(&tree);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].reason, "setgid");
    }

    #[test]
    fn scan_suspicious_flags_world_writable_file() {
        let mut tree = FileTree::new();
        tree.insert_node("/tmp/scratch", file_node("/tmp/scratch", 0o100666));

        let findings = scan_suspicious(&tree);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].reason, "world_writable");
    }

    #[test]
    fn scan_suspicious_does_not_flag_directory_with_suspicious_mode_bits() {
        let mut tree = FileTree::new();
        tree.insert_node("/tmp", dir_node("/tmp", 0o104777));

        let findings = scan_suspicious(&tree);

        assert!(findings.is_empty());
    }

    #[test]
    fn scan_suspicious_flags_secret_pattern_filenames() {
        let mut tree = FileTree::new();
        tree.insert_node(
            "/root/.ssh/id_rsa",
            file_node("/root/.ssh/id_rsa", 0o100600),
        );
        tree.insert_node(
            "/etc/tls/server.pem",
            file_node("/etc/tls/server.pem", 0o100644),
        );
        tree.insert_node("/app/.env", file_node("/app/.env", 0o100644));
        tree.insert_node("/app/keys.txt", file_node("/app/keys.txt", 0o100644));

        let findings = scan_suspicious(&tree);
        let secret_paths: Vec<&str> = findings
            .iter()
            .filter(|f| f.reason == "secret_pattern")
            .map(|f| f.path.as_str())
            .collect();

        assert!(secret_paths.contains(&"/root/.ssh/id_rsa"));
        assert!(secret_paths.contains(&"/etc/tls/server.pem"));
        assert!(secret_paths.contains(&"/app/.env"));
        assert!(!secret_paths.contains(&"/app/keys.txt"));
    }

    #[test]
    fn scan_suspicious_emits_two_findings_for_file_matching_two_rules() {
        let mut tree = FileTree::new();
        tree.insert_node(
            "/etc/secrets/server.key",
            file_node("/etc/secrets/server.key", 0o104644),
        );

        let findings = scan_suspicious(&tree);
        let reasons: Vec<&str> = findings.iter().map(|f| f.reason).collect();

        assert_eq!(findings.len(), 2);
        assert!(reasons.contains(&"setuid"));
        assert!(reasons.contains(&"secret_pattern"));
    }

    #[test]
    fn scan_suspicious_does_not_flag_symlink_with_secret_name() {
        let mut tree = FileTree::new();
        tree.insert_node(
            "/root/.ssh/id_rsa",
            symlink_node("/root/.ssh/id_rsa", "/other/location"),
        );

        let findings = scan_suspicious(&tree);

        assert!(findings.is_empty());
    }

    #[test]
    fn scan_suspicious_does_not_flag_symlink_with_suspicious_mode() {
        let mut tree = FileTree::new();
        // Create a symlink but manually set mode bits that would be suspicious for regular files
        let mut symlink = symlink_node("/usr/bin/symlink", "/target");
        symlink.mode = 0o104755; // setuid bits (would trigger setuid rule if symlink check weren't in place)
        tree.insert_node("/usr/bin/symlink", symlink);

        let findings = scan_suspicious(&tree);

        assert!(findings.is_empty());
    }

    #[test]
    fn scan_suspicious_does_not_flag_symlink_with_secret_extension() {
        let mut tree = FileTree::new();
        tree.insert_node(
            "/app/secret.pem",
            symlink_node("/app/secret.pem", "/etc/ssl/certs/secret.pem"),
        );

        let findings = scan_suspicious(&tree);

        assert!(findings.is_empty());
    }
}
