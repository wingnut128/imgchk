package extract

import (
	"archive/tar"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	v1 "github.com/google/go-containerregistry/pkg/v1"
	"github.com/google/go-containerregistry/pkg/v1/mutate"
)

// ExtractFiles extracts specified file paths from the flattened image filesystem.
// Uses mutate.Extract which handles whiteout semantics correctly.
func ExtractFiles(img v1.Image, paths []string, outputDir string) error {
	rc := mutate.Extract(img)
	defer rc.Close()

	return extractFromTar(rc, paths, outputDir)
}

// ExtractFromLayer extracts specific files from a single layer's tar.
func ExtractFromLayer(layer v1.Layer, paths []string, outputDir string) error {
	rc, err := layer.Uncompressed()
	if err != nil {
		return fmt.Errorf("reading layer: %w", err)
	}
	defer rc.Close()

	return extractFromTar(rc, paths, outputDir)
}

func extractFromTar(r io.Reader, paths []string, outputDir string) error {
	wanted := make(map[string]bool, len(paths))
	for _, p := range paths {
		wanted[p] = true
	}

	// Also collect parent directories we'll need
	needDirs := make(map[string]bool)
	for _, p := range paths {
		dir := filepath.Dir(p)
		for dir != "/" && dir != "." {
			needDirs[dir] = true
			dir = filepath.Dir(dir)
		}
	}

	tr := tar.NewReader(r)
	extracted := 0

	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("reading tar: %w", err)
		}

		cleanPath := filepath.Clean("/" + hdr.Name)

		if !wanted[cleanPath] {
			continue
		}

		destPath := filepath.Join(outputDir, cleanPath)

		// Ensure the destination is within outputDir (prevent path traversal)
		if !strings.HasPrefix(filepath.Clean(destPath), filepath.Clean(outputDir)) {
			continue
		}

		if err := os.MkdirAll(filepath.Dir(destPath), 0o755); err != nil {
			return fmt.Errorf("creating directory for %s: %w", cleanPath, err)
		}

		switch hdr.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(destPath, hdr.FileInfo().Mode()); err != nil {
				return fmt.Errorf("creating directory %s: %w", cleanPath, err)
			}
		case tar.TypeSymlink:
			if err := os.Symlink(hdr.Linkname, destPath); err != nil {
				return fmt.Errorf("creating symlink %s: %w", cleanPath, err)
			}
		case tar.TypeLink:
			linkPath := filepath.Join(outputDir, filepath.Clean("/"+hdr.Linkname))
			if err := os.Link(linkPath, destPath); err != nil {
				// Fall back to copying if hard link fails
				if err := copyFromTarEntry(tr, destPath, hdr); err != nil {
					return fmt.Errorf("creating hard link %s: %w", cleanPath, err)
				}
			}
		default:
			if err := writeFile(destPath, tr, hdr.FileInfo().Mode()); err != nil {
				return fmt.Errorf("writing %s: %w", cleanPath, err)
			}
		}

		extracted++
		if extracted >= len(wanted) {
			break // Found all requested files
		}
	}

	return nil
}

func writeFile(path string, r io.Reader, mode os.FileMode) error {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, mode)
	if err != nil {
		return err
	}
	defer f.Close()

	_, err = io.Copy(f, r)
	return err
}

func copyFromTarEntry(tr *tar.Reader, destPath string, hdr *tar.Header) error {
	return writeFile(destPath, tr, hdr.FileInfo().Mode())
}
