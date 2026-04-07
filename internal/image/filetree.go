package image

import (
	"archive/tar"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	v1 "github.com/google/go-containerregistry/pkg/v1"
)

// FileNode represents a file or directory in the image filesystem.
type FileNode struct {
	Name       string
	Path       string
	Size       int64
	Mode       os.FileMode
	IsDir      bool
	IsWhiteout bool
	IsOpaque   bool
	LinkTarget string
	Children   map[string]*FileNode
}

// FileTree is the root of a layer's filesystem.
type FileTree struct {
	Root      *FileNode
	FileCount int
	TotalSize int64
}

// NewFileTree creates an empty file tree.
func NewFileTree() *FileTree {
	return &FileTree{
		Root: &FileNode{
			Name:     "/",
			Path:     "/",
			IsDir:    true,
			Children: make(map[string]*FileNode),
		},
	}
}

// ParseLayerFileTree reads the uncompressed tar stream from a layer
// and constructs a FileTree.
func ParseLayerFileTree(layer v1.Layer) (*FileTree, error) {
	rc, err := layer.Uncompressed()
	if err != nil {
		return nil, err
	}
	defer rc.Close()

	tree := NewFileTree()
	tr := tar.NewReader(rc)

	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, err
		}

		path := filepath.Clean("/" + hdr.Name)
		name := filepath.Base(path)

		isWhiteout := strings.HasPrefix(name, ".wh.")
		isOpaque := name == ".wh..wh..opq"

		node := &FileNode{
			Name:       name,
			Path:       path,
			Size:       hdr.Size,
			Mode:       hdr.FileInfo().Mode(),
			IsDir:      hdr.Typeflag == tar.TypeDir,
			IsWhiteout: isWhiteout,
			IsOpaque:   isOpaque,
			LinkTarget: hdr.Linkname,
		}

		if hdr.Typeflag == tar.TypeSymlink || hdr.Typeflag == tar.TypeLink {
			node.LinkTarget = hdr.Linkname
		}

		tree.addNode(path, node)

		if !node.IsDir {
			tree.FileCount++
			tree.TotalSize += hdr.Size
		}
	}

	return tree, nil
}

func (t *FileTree) addNode(path string, node *FileNode) {
	dir := filepath.Dir(path)
	parent := t.ensureDir(dir)

	if node.IsDir {
		if existing, ok := parent.Children[node.Name]; ok {
			existing.Mode = node.Mode
			return
		}
		node.Children = make(map[string]*FileNode)
	}

	parent.Children[node.Name] = node
}

func (t *FileTree) ensureDir(path string) *FileNode {
	if path == "/" || path == "." {
		return t.Root
	}

	parts := strings.Split(strings.TrimPrefix(path, "/"), "/")
	current := t.Root

	for i, part := range parts {
		child, ok := current.Children[part]
		if !ok {
			child = &FileNode{
				Name:     part,
				Path:     "/" + strings.Join(parts[:i+1], "/"),
				IsDir:    true,
				Children: make(map[string]*FileNode),
			}
			current.Children[part] = child
		}
		current = child
	}

	return current
}

// MergeTrees combines multiple layer trees into a cumulative view,
// applying whiteout semantics.
func MergeTrees(trees ...*FileTree) *FileTree {
	merged := NewFileTree()

	for _, tree := range trees {
		applyLayer(merged.Root, tree.Root)
	}

	recount(merged)
	return merged
}

func applyLayer(target, source *FileNode) {
	for name, srcChild := range source.Children {
		if srcChild.IsOpaque {
			// Opaque whiteout: clear all existing children in this directory
			target.Children = make(map[string]*FileNode)
			continue
		}

		if srcChild.IsWhiteout {
			// Delete the corresponding file/dir
			deleteName := strings.TrimPrefix(name, ".wh.")
			delete(target.Children, deleteName)
			continue
		}

		if srcChild.IsDir {
			existing, ok := target.Children[name]
			if !ok || !existing.IsDir {
				existing = &FileNode{
					Name:     srcChild.Name,
					Path:     srcChild.Path,
					IsDir:    true,
					Mode:     srcChild.Mode,
					Children: make(map[string]*FileNode),
				}
				target.Children[name] = existing
			}
			applyLayer(existing, srcChild)
		} else {
			target.Children[name] = &FileNode{
				Name:       srcChild.Name,
				Path:       srcChild.Path,
				Size:       srcChild.Size,
				Mode:       srcChild.Mode,
				IsDir:      false,
				LinkTarget: srcChild.LinkTarget,
			}
		}
	}
}

func recount(tree *FileTree) {
	tree.FileCount = 0
	tree.TotalSize = 0
	countNode(tree.Root, tree)
}

func countNode(node *FileNode, tree *FileTree) {
	for _, child := range node.Children {
		if child.IsDir {
			countNode(child, tree)
		} else {
			tree.FileCount++
			tree.TotalSize += child.Size
		}
	}
}

// SortedChildren returns the children of a node sorted alphabetically,
// directories first.
func SortedChildren(node *FileNode) []*FileNode {
	children := make([]*FileNode, 0, len(node.Children))
	for _, child := range node.Children {
		children = append(children, child)
	}
	sort.Slice(children, func(i, j int) bool {
		if children[i].IsDir != children[j].IsDir {
			return children[i].IsDir
		}
		return children[i].Name < children[j].Name
	})
	return children
}

// CollectAllPaths returns all file paths under the given node recursively.
func CollectAllPaths(node *FileNode) []string {
	var paths []string
	collectPaths(node, &paths)
	return paths
}

func collectPaths(node *FileNode, paths *[]string) {
	if !node.IsDir {
		*paths = append(*paths, node.Path)
		return
	}
	for _, child := range node.Children {
		collectPaths(child, paths)
	}
}
