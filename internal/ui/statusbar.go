package ui

import (
	"fmt"
	"strings"
)

// StatusBarModel displays keybinding hints and messages.
type StatusBarModel struct {
	message       string
	width         int
	selectedCount int
	cumulative    bool
}

// NewStatusBarModel creates a new status bar model.
func NewStatusBarModel(width int) StatusBarModel {
	return StatusBarModel{width: width}
}

// SetMessage sets a transient message.
func (m *StatusBarModel) SetMessage(msg string) {
	m.message = msg
}

// ClearMessage clears the transient message.
func (m *StatusBarModel) ClearMessage() {
	m.message = ""
}

// SetSize updates the status bar width.
func (m *StatusBarModel) SetSize(width int) {
	m.width = width
}

// View renders the status bar.
func (m StatusBarModel) View() string {
	if m.width <= 0 {
		return ""
	}

	var left string
	if m.message != "" {
		left = " " + m.message
	} else {
		helps := []struct{ key, desc string }{
			{"tab", "pane"},
			{"↑↓", "nav"},
			{"space", "select"},
			{"enter", "expand"},
			{"t", "view"},
			{"e", "extract"},
			{"q", "quit"},
		}
		var parts []string
		for _, h := range helps {
			parts = append(parts, HelpKeyStyle.Render(h.key)+HelpDescStyle.Render(":"+h.desc))
		}
		left = " " + strings.Join(parts, "  ")
	}

	var right string
	if m.selectedCount > 0 {
		right = fmt.Sprintf(" %d selected ", m.selectedCount)
	}

	// Pad middle
	leftLen := len(stripAnsi(left))
	rightLen := len(stripAnsi(right))
	pad := m.width - leftLen - rightLen
	if pad < 0 {
		pad = 0
	}

	line := left + strings.Repeat(" ", pad) + right
	return StatusBarStyle.Width(m.width).Render(line)
}

// stripAnsi removes ANSI escape codes for width calculation.
func stripAnsi(s string) string {
	var result strings.Builder
	inEscape := false
	for _, r := range s {
		if r == '\033' {
			inEscape = true
			continue
		}
		if inEscape {
			if (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') {
				inEscape = false
			}
			continue
		}
		result.WriteRune(r)
	}
	return result.String()
}
