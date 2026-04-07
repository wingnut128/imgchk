package image

import (
	"fmt"
	"time"

	v1 "github.com/google/go-containerregistry/pkg/v1"
)

// LayerInfo holds pre-parsed metadata for a single layer.
type LayerInfo struct {
	Index    int
	Digest   v1.Hash
	DiffID   v1.Hash
	Size     int64
	Command  string
	Created  time.Time
	Empty    bool
	FileTree *FileTree
	Layer    v1.Layer
}

// ImageInfo holds the complete analyzed image.
type ImageInfo struct {
	Layers       []LayerInfo
	TotalSize    int64
	Architecture string
	OS           string
	Created      time.Time
	Image        v1.Image
}

// Analyze parses all metadata from an image.
func Analyze(img v1.Image) (*ImageInfo, error) {
	cfg, err := img.ConfigFile()
	if err != nil {
		return nil, fmt.Errorf("reading config: %w", err)
	}

	layers, err := img.Layers()
	if err != nil {
		return nil, fmt.Errorf("reading layers: %w", err)
	}

	// Correlate history entries (skipping empty layers) with actual layers
	commands := make([]string, len(layers))
	createdTimes := make([]time.Time, len(layers))
	if cfg.History != nil {
		layerIdx := 0
		for _, h := range cfg.History {
			if h.EmptyLayer {
				continue
			}
			if layerIdx < len(layers) {
				commands[layerIdx] = h.CreatedBy
				if !h.Created.IsZero() {
					createdTimes[layerIdx] = h.Created.Time
				}
				layerIdx++
			}
		}
	}

	info := &ImageInfo{
		Layers:       make([]LayerInfo, len(layers)),
		Architecture: cfg.Architecture,
		OS:           cfg.OS,
		Image:        img,
	}

	if !cfg.Created.IsZero() {
		info.Created = cfg.Created.Time
	}

	for i, layer := range layers {
		digest, _ := layer.Digest()
		diffID, _ := layer.DiffID()
		size, _ := layer.Size()

		ft, err := ParseLayerFileTree(layer)
		if err != nil {
			return nil, fmt.Errorf("parsing layer %d file tree: %w", i, err)
		}

		info.Layers[i] = LayerInfo{
			Index:    i,
			Digest:   digest,
			DiffID:   diffID,
			Size:     size,
			Command:  commands[i],
			Created:  createdTimes[i],
			FileTree: ft,
			Layer:    layer,
		}
		info.TotalSize += size
	}

	return info, nil
}

// HumanSize returns a human-readable size string.
func HumanSize(bytes int64) string {
	const (
		KB = 1024
		MB = KB * 1024
		GB = MB * 1024
	)
	switch {
	case bytes >= GB:
		return fmt.Sprintf("%.1f GB", float64(bytes)/float64(GB))
	case bytes >= MB:
		return fmt.Sprintf("%.1f MB", float64(bytes)/float64(MB))
	case bytes >= KB:
		return fmt.Sprintf("%.1f KB", float64(bytes)/float64(KB))
	default:
		return fmt.Sprintf("%d B", bytes)
	}
}

// TruncateString truncates a string to max length with ellipsis.
func TruncateString(s string, max int) string {
	if len(s) <= max {
		return s
	}
	if max <= 3 {
		return s[:max]
	}
	return s[:max-3] + "..."
}
