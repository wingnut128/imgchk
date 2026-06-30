use serde::Serialize;

use crate::command_format::clean_command;
use crate::image::ImageInfo;
use crate::tree::FileTree;

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

pub fn scan_suspicious(_tree: &FileTree) -> Vec<SuspiciousFile> {
    Vec::new() // implemented in Task 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::LayerInfo;
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
}
