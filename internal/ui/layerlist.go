package ui

import (
	"fmt"

	"imgchk/internal/image"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

// LayerListModel displays the list of image layers.
type LayerListModel struct {
	layers   []image.LayerInfo
	cursor   int
	width    int
	height   int
	offset   int // scroll offset
}

// NewLayerListModel creates a new layer list model.
func NewLayerListModel(layers []image.LayerInfo, width, height int) LayerListModel {
	return LayerListModel{
		layers: layers,
		width:  width,
		height: height,
	}
}

// SelectedIndex returns the currently selected layer index.
func (m LayerListModel) SelectedIndex() int {
	return m.cursor
}

// SetSize updates the pane dimensions.
func (m *LayerListModel) SetSize(width, height int) {
	m.width = width
	m.height = height
}

// Update handles key messages for the layer list.
func (m LayerListModel) Update(msg tea.Msg) (LayerListModel, tea.Cmd) {
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
			if m.cursor < len(m.layers)-1 {
				m.cursor++
				visibleRows := m.height - 2 // account for title
				if visibleRows > 0 && m.cursor >= m.offset+visibleRows {
					m.offset = m.cursor - visibleRows + 1
				}
			}
		}
	}
	return m, nil
}

// View renders the layer list.
func (m LayerListModel) View() string {
	if m.width <= 0 || m.height <= 0 {
		return ""
	}

	contentWidth := m.width - 2 // borders
	if contentWidth < 1 {
		contentWidth = 1
	}

	var lines []string
	lines = append(lines, TitleStyle.Render("Layers"))

	visibleRows := m.height - 2 // title + padding
	if visibleRows < 1 {
		visibleRows = 1
	}

	end := m.offset + visibleRows
	if end > len(m.layers) {
		end = len(m.layers)
	}

	for i := m.offset; i < end; i++ {
		layer := m.layers[i]
		size := image.HumanSize(layer.Size)
		cmd := image.TruncateString(layer.Command, contentWidth-16)
		if cmd == "" {
			cmd = "(empty)"
		}

		line := fmt.Sprintf(" %d  %7s  %s", layer.Index, size, cmd)
		if len(line) > contentWidth {
			line = line[:contentWidth]
		}

		// Pad to full width
		for len(line) < contentWidth {
			line += " "
		}

		if i == m.cursor {
			line = LayerSelectedStyle.Render(line)
		} else {
			line = LayerNormalStyle.Render(line)
		}

		lines = append(lines, line)
	}

	// Pad remaining height
	for len(lines) < m.height {
		lines = append(lines, "")
	}

	content := lipgloss.JoinVertical(lipgloss.Left, lines...)
	return content
}
