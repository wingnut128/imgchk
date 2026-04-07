package ui

import (
	"fmt"
	"strings"

	"imgchk/internal/image"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

// DetailsModel shows metadata about the currently selected layer.
type DetailsModel struct {
	content string
	width   int
	height  int
	offset  int
	lines   []string
}

// NewDetailsModel creates a new details model.
func NewDetailsModel(width, height int) DetailsModel {
	return DetailsModel{
		width:  width,
		height: height,
	}
}

// SetLayer updates the displayed details for a layer.
func (m *DetailsModel) SetLayer(layer image.LayerInfo) {
	var b strings.Builder

	b.WriteString(DetailLabelStyle.Render("Command:") + "  ")
	if layer.Command != "" {
		b.WriteString(DetailValueStyle.Render(layer.Command))
	} else {
		b.WriteString(DetailValueStyle.Render("(none)"))
	}
	b.WriteString("\n")

	b.WriteString(DetailLabelStyle.Render("Digest:") + "   ")
	b.WriteString(DetailValueStyle.Render(layer.Digest.String()))
	b.WriteString("\n")

	b.WriteString(DetailLabelStyle.Render("DiffID:") + "   ")
	b.WriteString(DetailValueStyle.Render(layer.DiffID.String()))
	b.WriteString("\n")

	b.WriteString(DetailLabelStyle.Render("Size:") + "     ")
	b.WriteString(DetailValueStyle.Render(image.HumanSize(layer.Size)))
	b.WriteString("\n")

	if !layer.Created.IsZero() {
		b.WriteString(DetailLabelStyle.Render("Created:") + "  ")
		b.WriteString(DetailValueStyle.Render(layer.Created.Format("2006-01-02 15:04:05")))
		b.WriteString("\n")
	}

	if layer.FileTree != nil {
		b.WriteString(DetailLabelStyle.Render("Files:") + "    ")
		b.WriteString(DetailValueStyle.Render(
			fmt.Sprintf("%d files, %s total", layer.FileTree.FileCount, image.HumanSize(layer.FileTree.TotalSize)),
		))
		b.WriteString("\n")
	}

	m.content = b.String()
	m.lines = strings.Split(m.content, "\n")
	m.offset = 0
}

// SetSize updates the pane dimensions.
func (m *DetailsModel) SetSize(width, height int) {
	m.width = width
	m.height = height
}

// Update handles key messages for the details pane.
func (m DetailsModel) Update(msg tea.Msg) (DetailsModel, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "up", "k":
			if m.offset > 0 {
				m.offset--
			}
		case "down", "j":
			maxOffset := len(m.lines) - m.height + 2
			if maxOffset < 0 {
				maxOffset = 0
			}
			if m.offset < maxOffset {
				m.offset++
			}
		}
	}
	return m, nil
}

// View renders the details panel.
func (m DetailsModel) View() string {
	if m.width <= 0 || m.height <= 0 {
		return ""
	}

	var lines []string
	lines = append(lines, TitleStyle.Render("Details"))

	visibleRows := m.height - 2
	if visibleRows < 1 {
		visibleRows = 1
	}

	end := m.offset + visibleRows
	if end > len(m.lines) {
		end = len(m.lines)
	}

	for i := m.offset; i < end; i++ {
		lines = append(lines, " "+m.lines[i])
	}

	for len(lines) < m.height {
		lines = append(lines, "")
	}

	return lipgloss.JoinVertical(lipgloss.Left, lines...)
}
