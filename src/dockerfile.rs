use crate::command_format::clean_command;
use crate::image::HistoryStep;

const HEADER: &str = "\
# Reconstructed by imgchk from image build history.
# This is an approximation, NOT a guaranteed-buildable Dockerfile:
#   - the base image (FROM) cannot be recovered from history
#   - COPY/ADD build context is not stored in the image
# Review before use.
";

const INSTRUCTIONS: &[&str] = &[
    "RUN",
    "CMD",
    "ENV",
    "ENTRYPOINT",
    "EXPOSE",
    "WORKDIR",
    "USER",
    "LABEL",
    "VOLUME",
    "ARG",
    "MAINTAINER",
    "COPY",
    "ADD",
    "HEALTHCHECK",
    "STOPSIGNAL",
    "SHELL",
    "ONBUILD",
];

/// Render the full history as an approximate, annotated Dockerfile.
pub fn reconstruct(history: &[HistoryStep]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for step in history {
        let norm = normalize(&step.created_by);
        if norm.is_empty() {
            continue;
        }
        if let Some(rewritten) = rewrite_legacy_copy_add(&norm) {
            lines.push(rewritten);
        } else if starts_with_instruction(&norm) {
            lines.push(norm);
        } else {
            lines.push(format!("RUN {norm}"));
        }
    }
    if lines.is_empty() {
        return format!(
            "{HEADER}# No build history available in this image (squashed or history-stripped).\n"
        );
    }
    format!("{HEADER}{}\n", lines.join("\n"))
}

/// Render the verbatim ordered command list (one created_by per line).
#[allow(dead_code)]
pub fn render_raw(history: &[HistoryStep]) -> String {
    if history.is_empty() {
        return "# No build history available in this image.".to_string();
    }
    history
        .iter()
        .map(|s| s.created_by.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip `/bin/sh -c`, `#(nop)`, collapse whitespace (via clean_command), then
/// drop a trailing BuildKit ` # buildkit` marker.
fn normalize(created_by: &str) -> String {
    let cleaned = clean_command(created_by);
    cleaned
        .strip_suffix("# buildkit")
        .unwrap_or(&cleaned)
        .trim()
        .to_string()
}

fn starts_with_instruction(line: &str) -> bool {
    let first = line.split_whitespace().next().unwrap_or("");
    INSTRUCTIONS.iter().any(|k| k.eq_ignore_ascii_case(first))
}

/// Detect the legacy `COPY dir:<hash> in <dest>` / `ADD file:<hash> in <dest>`
/// form (build context not recoverable) and rewrite it to an annotated line.
/// Returns None for BuildKit lines that carry a real source path.
fn rewrite_legacy_copy_add(line: &str) -> Option<String> {
    let inst = if line.starts_with("COPY ") {
        "COPY"
    } else if line.starts_with("ADD ") {
        "ADD"
    } else {
        return None;
    };
    let rest = &line[inst.len() + 1..];
    let idx = rest.find(" in ")?;
    let src = &rest[..idx];
    let dest = rest[idx + " in ".len()..].trim();
    // Legacy source looks like "dir:<hex>" / "file:<hex>" / "multi:<hex>".
    let looks_legacy = src
        .split_once(':')
        .map(|(_, hex)| !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or(false);
    if looks_legacy {
        Some(format!(
            "{inst} <context unavailable> {dest}  # reconstructed: original source not in image"
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(created_by: &str, empty: bool) -> HistoryStep {
        HistoryStep {
            created_by: created_by.to_string(),
            empty_layer: empty,
            created: String::new(),
        }
    }

    #[test]
    fn reconstruct_maps_nop_env_and_plain_run() {
        let history = vec![
            step("/bin/sh -c #(nop)  ENV PATH=/usr/local/bin", true),
            step(
                "/bin/sh -c apt-get update && apt-get install -y nginx",
                false,
            ),
        ];
        let out = reconstruct(&history);
        assert!(out.contains("ENV PATH=/usr/local/bin"));
        assert!(out.contains("RUN apt-get update && apt-get install -y nginx"));
        // Header present.
        assert!(out.contains("Reconstructed by imgchk"));
        assert!(out.contains("NOT a guaranteed-buildable"));
    }

    #[test]
    fn reconstruct_rewrites_legacy_copy() {
        let history = vec![step("/bin/sh -c #(nop) COPY dir:abc123def in /app", false)];
        let out = reconstruct(&history);
        assert!(out.contains("COPY <context unavailable> /app"));
        assert!(out.contains("original source not in image"));
        assert!(!out.contains("dir:abc123def"));
    }

    #[test]
    fn reconstruct_strips_buildkit_marker() {
        let history = vec![step("RUN /bin/sh -c apk add curl # buildkit", false)];
        let out = reconstruct(&history);
        assert!(out.contains("RUN apk add curl"));
        assert!(!out.contains("buildkit"));
    }

    #[test]
    fn reconstruct_passes_through_buildkit_real_copy() {
        // BuildKit can preserve a real source path — must NOT be rewritten.
        let history = vec![step("COPY ./app /app # buildkit", false)];
        let out = reconstruct(&history);
        assert!(out.contains("COPY ./app /app"));
        assert!(!out.contains("context unavailable"));
    }

    #[test]
    fn reconstruct_empty_history_notes_it() {
        let out = reconstruct(&[]);
        assert!(out.contains("No build history available"));
        assert!(out.contains("Reconstructed by imgchk"));
    }

    #[test]
    fn render_raw_is_verbatim_and_ordered() {
        let history = vec![
            step("/bin/sh -c #(nop)  ENV A=1", true),
            step("/bin/sh -c apt-get update", false),
        ];
        let out = render_raw(&history);
        assert_eq!(out, "/bin/sh -c #(nop)  ENV A=1\n/bin/sh -c apt-get update");
    }

    #[test]
    fn render_raw_empty_history() {
        assert!(render_raw(&[]).contains("No build history available"));
    }
}
