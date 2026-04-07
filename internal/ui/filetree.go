package ui

import (
	"fmt"

	"imgchk/internal/image"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

// TreeEntry is a flattened row in the visible file tree.
type TreeEntry struct {
	Node     *image.FileNode
	Depth    int
	IsLast   bool
	Prefix   string // pre-computed tree drawing prefix
}

// FileTreeModel displays a navigable file tree.
type FileTreeModel struct {
	flatEntries   []TreeEntry
	cursor        int
	tree          *image.FileTree
	expanded      map[string]bool
	selectedFiles map[string]bool
	cumulative    bool
	width         int
	height        int
	offset        int
}

// NewFileTreeModel creates a new file tree model.
func NewFileTreeModel(width, height int, selectedFiles map[string]bool) FileTreeModel {
	return FileTreeModel{
		expanded:      make(map[string]bool),
		selectedFiles: selectedFiles,
		width:         width,
		height:        height,
	}
}

// SetTree replaces the displayed tree.
func (m *FileTreeModel) SetTree(tree *image.FileTree, cumulative bool) {
	m.tree = tree
	m.cumulative = cumulative
	m.cursor = 0
	m.offset = 0
	m.rebuildFlat()
}

// SetSize updates the pane dimensions.
func (m *FileTreeModel) SetSize(width, height int) {
	m.width = width
	m.height = height
}

func (m *FileTreeModel) rebuildFlat() {
	m.flatEntries = nil
	if m.tree == nil {
		return
	}

	children := image.SortedChildren(m.tree.Root)
	for i, child := range children {
		isLast := i == len(children)-1
		m.flattenNode(child, 0, isLast, "")
	}
}

func (m *FileTreeModel) flattenNode(node *image.FileNode, depth int, isLast bool, parentPrefix string) {
	// In cumulative view, hide whiteout markers
	if m.cumulative && (node.IsWhiteout || node.IsOpaque) {
		return
	}

	var connector string
	if depth == 0 {
		connector = ""
	} else if isLast {
		connector = "└── "
	} else {
		connector = "├── "
	}

	prefix := parentPrefix + connector

	m.flatEntries = append(m.flatEntries, TreeEntry{
		Node:   node,
		Depth:  depth,
		IsLast: isLast,
		Prefix: prefix,
	})

	if node.IsDir && m.expanded[node.Path] {
		children := image.SortedChildren(node)
		var childPrefix string
		if depth == 0 {
			childPrefix = parentPrefix
		} else if isLast {
			childPrefix = parentPrefix + "    "
		} else {
			childPrefix = parentPrefix + "│   "
		}

		for i, child := range children {
			childIsLast := i == len(children)-1
			m.flattenNode(child, depth+1, childIsLast, childPrefix)
		}
	}
}

// Update handles key messages for the file tree.
func (m FileTreeModel) Update(msg tea.Msg) (FileTreeModel, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "up", "k":
			if m.cursor > 0 {
				m.cursor--
				if m.cursor < m.offset {
					m.offset = m.cursor
				}
			}
		case "down", "j":
			if m.cursor < len(m.flatEntries)-1 {
				m.cursor++
				visibleRows := m.height - 2
				if visibleRows > 0 && m.cursor >= m.offset+visibleRows {
					m.offset = m.cursor - visibleRows + 1
				}
			}
		case "enter":
			if m.cursor < len(m.flatEntries) {
				entry := m.flatEntries[m.cursor]
				if entry.Node.IsDir {
					if m.expanded[entry.Node.Path] {
						delete(m.expanded, entry.Node.Path)
					} else {
						m.expanded[entry.Node.Path] = true
					}
					m.rebuildFlat()
					// Keep cursor in bounds
					if m.cursor >= len(m.flatEntries) {
						m.cursor = len(m.flatEntries) - 1
					}
				}
			}
		case " ":
			if m.cursor < len(m.flatEntries) {
				entry := m.flatEntries[m.cursor]
				if entry.Node.IsDir {
					// Toggle all files under this directory
					paths := image.CollectAllPaths(entry.Node)
					allSelected := true
					for _, p := range paths {
						if !m.selectedFiles[p] {
							allSelected = false
							break
						}
					}
					for _, p := range paths {
						if allSelected {
							delete(m.selectedFiles, p)
						} else {
							m.selectedFiles[p] = true
						}
					}
				} else {
					if m.selectedFiles[entry.Node.Path] {
						delete(m.selectedFiles, entry.Node.Path)
					} else {
						m.selectedFiles[entry.Node.Path] = true
					}
				}
			}
		}
	}
	return m, nil
}

// View renders the file tree.
func (m FileTreeModel) View() string {
	if m.width <= 0 || m.height <= 0 {
		return ""
	}

	contentWidth := m.width - 2
	if contentWidth < 1 {
		contentWidth = 1
	}

	var lines []string

	// Title with view mode badge
	var badge string
	if m.cumulative {
		badge = CumulativeBadge.Render("CUMULATIVE")
	} else {
		badge = SingleLayerBadge.Render("LAYER")
	}
	title := TitleStyle.Render("Files") + " " + badge
	lines = append(lines, title)

	if len(m.flatEntries) == 0 {
		lines = append(lines, "  (empty layer)")
		for len(lines) < m.height {
			lines = append(lines, "")
		}
		return lipgloss.JoinVertical(lipgloss.Left, lines...)
	}

	visibleRows := m.height - 2
	if visibleRows < 1 {
		visibleRows = 1
	}

	end := m.offset + visibleRows
	if end > len(m.flatEntries) {
		end = len(m.flatEntries)
	}

	for i := m.offset; i < end; i++ {
		entry := m.flatEntries[i]
		line := m.renderEntry(entry, i == m.cursor, contentWidth)
		lines = append(lines, line)
	}

	for len(lines) < m.height {
		lines = append(lines, "")
	}

	return lipgloss.JoinVertical(lipgloss.Left, lines...)
}

func (m FileTreeModel) renderEntry(entry TreeEntry, isCursor bool, maxWidth int) string {
	node := entry.Node

	// Selection marker
	var marker string
	if node.IsDir {
		paths := image.CollectAllPaths(node)
		allSelected := len(paths) > 0
		anySelected := false
		for _, p := range paths {
			if m.selectedFiles[p] {
				anySelected = true
			} else {
				allSelected = false
			}
		}
		if allSelected {
			marker = SelectedMarkerStyle.Render("[x]")
		} else if anySelected {
			marker = SelectedMarkerStyle.Render("[-]")
		} else {
			marker = UnselectedMarkerStyle.Render("[ ]")
		}
	} else {
		if m.selectedFiles[node.Path] {
			marker = SelectedMarkerStyle.Render("[x]")
		} else {
			marker = UnselectedMarkerStyle.Render("[ ]")
		}
	}

	// Name with styling
	var name string
	if node.IsWhiteout {
		name = WhiteoutStyle.Render(node.Name)
	} else if node.IsDir {
		name = DirStyle.Render(node.Name + "/")
	} else if node.LinkTarget != "" {
		name = SymlinkStyle.Render(node.Name + " -> " + node.LinkTarget)
	} else {
		name = FileStyle.Render(node.Name)
	}

	// Size for files
	var sizeStr string
	if !node.IsDir && node.Size > 0 {
		sizeStr = " " + SizeStyle.Render(fmt.Sprintf("(%s)", image.HumanSize(node.Size)))
	}

	line := fmt.Sprintf(" %s %s%s %s%s", marker, entry.Prefix, name, sizeStr, "")

	// Truncate if needed (rough — lipgloss widths can differ)
	if len(line) > maxWidth+20 { // allow for ANSI codes
		line = line[:maxWidth+20]
	}

	if isCursor {
		// Wrap entire line in cursor background
		line = CursorStyle.Render(line)
	}

	return line
}

// SelectedCount returns the number of selected files.
func (m FileTreeModel) SelectedCount() int {
	return len(m.selectedFiles)
}
