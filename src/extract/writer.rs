use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::Compression;
use flate2::write::GzEncoder;

use super::path_safety::SafePath;

/// Sink for filtered tar entries during selective extraction.
///
/// Writers receive entries that have already passed both the
/// [`crate::extract::FileSelector`] and [`crate::extract::path_safety::safe_path`]
/// checks — i.e., the path is safe to use under the output root and the
/// entry has been chosen for inclusion.
///
/// Implementors are responsible for materializing whatever output shape
/// they represent (loose files, archive, etc.) and reporting how many
/// entries they actually wrote.
pub trait OutputWriter {
    /// Append one tar entry to the output. The reader yields the file
    /// contents. The writer is free to skip entries that don't match its
    /// shape (e.g., directory writers may treat entries with empty
    /// contents as mkdir-only).
    fn write_entry(
        &mut self,
        path: &SafePath,
        header: &tar::Header,
        contents: &mut dyn Read,
    ) -> anyhow::Result<()>;

    /// Flush and close the output. Returns paths created (loose files
    /// for [`DirWriter`]; the single archive path for the others).
    fn finish(self: Box<Self>) -> anyhow::Result<Vec<PathBuf>>;
}

// ── DirWriter ──────────────────────────────────────────────────────────────

/// Writes each entry as a loose file under the configured output dir.
pub struct DirWriter {
    output_dir: PathBuf,
    canonical_root: PathBuf,
    written: Vec<PathBuf>,
}

impl DirWriter {
    pub fn new(output_dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(output_dir)?;
        let canonical_root =
            std::fs::canonicalize(output_dir).unwrap_or_else(|_| output_dir.to_path_buf());
        Ok(Self {
            output_dir: output_dir.to_path_buf(),
            canonical_root,
            written: Vec::new(),
        })
    }
}

impl OutputWriter for DirWriter {
    fn write_entry(
        &mut self,
        path: &SafePath,
        _header: &tar::Header,
        contents: &mut dyn Read,
    ) -> anyhow::Result<()> {
        let dest = self.output_dir.join(&path.relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Symlink backstop: confirm the destination's resolved parent is
        // still under the output root. Catches malicious symlinks placed
        // earlier in the same archive.
        let parent_canonical = std::fs::canonicalize(dest.parent().unwrap_or(&self.output_dir))
            .unwrap_or_else(|_| self.output_dir.clone());
        if !parent_canonical.starts_with(&self.canonical_root) {
            return Ok(());
        }

        let mut out =
            std::fs::File::create(&dest).with_context(|| format!("creating {}", dest.display()))?;
        std::io::copy(contents, &mut out)?;
        self.written.push(dest);
        Ok(())
    }

    fn finish(self: Box<Self>) -> anyhow::Result<Vec<PathBuf>> {
        Ok(self.written)
    }
}

// ── TarWriter ──────────────────────────────────────────────────────────────

/// Writes entries into a single uncompressed tar archive.
pub struct TarWriter {
    builder: tar::Builder<std::fs::File>,
    output_path: PathBuf,
}

impl TarWriter {
    pub fn new(output_path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(&output_path)
            .with_context(|| format!("creating {}", output_path.display()))?;
        Ok(Self {
            builder: tar::Builder::new(file),
            output_path,
        })
    }
}

impl OutputWriter for TarWriter {
    fn write_entry(
        &mut self,
        path: &SafePath,
        header: &tar::Header,
        contents: &mut dyn Read,
    ) -> anyhow::Result<()> {
        let mut new_header = header.clone();
        new_header.set_path(&path.relative)?;
        new_header.set_cksum();
        self.builder.append(&new_header, contents)?;
        Ok(())
    }

    fn finish(self: Box<Self>) -> anyhow::Result<Vec<PathBuf>> {
        let mut builder = self.builder;
        builder.finish()?;
        drop(builder);
        Ok(vec![self.output_path])
    }
}

// ── TarGzWriter ────────────────────────────────────────────────────────────

/// Writes entries into a single gzip-compressed tar archive.
pub struct TarGzWriter {
    builder: tar::Builder<GzEncoder<std::fs::File>>,
    output_path: PathBuf,
}

impl TarGzWriter {
    pub fn new(output_path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(&output_path)
            .with_context(|| format!("creating {}", output_path.display()))?;
        let encoder = GzEncoder::new(file, Compression::default());
        Ok(Self {
            builder: tar::Builder::new(encoder),
            output_path,
        })
    }
}

impl OutputWriter for TarGzWriter {
    fn write_entry(
        &mut self,
        path: &SafePath,
        header: &tar::Header,
        contents: &mut dyn Read,
    ) -> anyhow::Result<()> {
        let mut new_header = header.clone();
        new_header.set_path(&path.relative)?;
        new_header.set_cksum();
        self.builder.append(&new_header, contents)?;
        Ok(())
    }

    fn finish(self: Box<Self>) -> anyhow::Result<Vec<PathBuf>> {
        let builder = self.builder;
        let encoder = builder.into_inner()?;
        let mut file = encoder.finish()?;
        file.flush()?;
        drop(file);
        Ok(vec![self.output_path])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn entry(path: &str, contents: &[u8]) -> (tar::Header, Vec<u8>) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        (header, contents.to_vec())
    }

    fn safe(p: &str) -> SafePath {
        crate::extract::path_safety::safe_path(p).unwrap()
    }

    #[test]
    fn dir_writer_writes_two_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut w: Box<dyn OutputWriter> = Box::new(DirWriter::new(dir.path()).unwrap());

        let (h1, c1) = entry("etc/hosts", b"127.0.0.1 localhost\n");
        let (h2, c2) = entry("usr/bin/ls", b"\x7fELF...");

        w.write_entry(&safe("/etc/hosts"), &h1, &mut Cursor::new(c1.clone()))
            .unwrap();
        w.write_entry(&safe("/usr/bin/ls"), &h2, &mut Cursor::new(c2.clone()))
            .unwrap();

        let written = w.finish().unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(std::fs::read(dir.path().join("etc/hosts")).unwrap(), c1);
        assert_eq!(std::fs::read(dir.path().join("usr/bin/ls")).unwrap(), c2);
    }

    #[test]
    fn tar_writer_produces_readable_archive() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("out.tar");
        let mut w: Box<dyn OutputWriter> = Box::new(TarWriter::new(out_path.clone()).unwrap());

        let (h1, c1) = entry("a.txt", b"alpha");
        let (h2, c2) = entry("nested/b.txt", b"bravo");
        w.write_entry(&safe("/a.txt"), &h1, &mut Cursor::new(c1))
            .unwrap();
        w.write_entry(&safe("/nested/b.txt"), &h2, &mut Cursor::new(c2))
            .unwrap();
        let written = w.finish().unwrap();
        assert_eq!(written, vec![out_path.clone()]);

        // Read the archive back and verify both entries.
        let mut archive = tar::Archive::new(std::fs::File::open(&out_path).unwrap());
        let mut found: Vec<(String, Vec<u8>)> = Vec::new();
        for e in archive.entries().unwrap() {
            let mut e = e.unwrap();
            let path = e.path().unwrap().to_string_lossy().into_owned();
            let mut buf = Vec::new();
            e.read_to_end(&mut buf).unwrap();
            found.push((path, buf));
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            found,
            vec![
                ("a.txt".to_string(), b"alpha".to_vec()),
                ("nested/b.txt".to_string(), b"bravo".to_vec()),
            ],
        );
    }

    #[test]
    fn tar_gz_writer_produces_readable_archive() {
        use flate2::read::GzDecoder;
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("out.tar.gz");
        let mut w: Box<dyn OutputWriter> = Box::new(TarGzWriter::new(out_path.clone()).unwrap());

        let (h1, c1) = entry("x.bin", b"hello world");
        w.write_entry(&safe("/x.bin"), &h1, &mut Cursor::new(c1))
            .unwrap();
        let written = w.finish().unwrap();
        assert_eq!(written, vec![out_path.clone()]);

        let file = std::fs::File::open(&out_path).unwrap();
        let mut archive = tar::Archive::new(GzDecoder::new(file));
        let mut entries = archive.entries().unwrap();
        let mut e = entries.next().unwrap().unwrap();
        let path = e.path().unwrap().to_string_lossy().into_owned();
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).unwrap();
        assert_eq!(path, "x.bin");
        assert_eq!(buf, b"hello world");
    }

    #[test]
    fn dir_writer_creates_intermediate_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let mut w: Box<dyn OutputWriter> = Box::new(DirWriter::new(dir.path()).unwrap());
        let (h, c) = entry("a/b/c/d.txt", b"deep");
        w.write_entry(&safe("/a/b/c/d.txt"), &h, &mut Cursor::new(c))
            .unwrap();
        let written = w.finish().unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(
            std::fs::read(dir.path().join("a/b/c/d.txt")).unwrap(),
            b"deep"
        );
    }
}
