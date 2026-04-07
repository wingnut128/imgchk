package ui

import "github.com/charmbracelet/lipgloss"

var (
	// Border styles
	ActiveBorderStyle = lipgloss.NewStyle().
				Border(lipgloss.RoundedBorder()).
				BorderForeground(lipgloss.Color("62"))

	InactiveBorderStyle = lipgloss.NewStyle().
				Border(lipgloss.RoundedBorder()).
				BorderForeground(lipgloss.Color("240"))

	// Layer list
	LayerSelectedStyle = lipgloss.NewStyle().
				Bold(true).
				Foreground(lipgloss.Color("229")).
				Background(lipgloss.Color("62"))

	LayerNormalStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("252"))

	// File tree
	DirStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("75"))

	FileStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("252"))

	WhiteoutStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("196")).
			Strikethrough(true)

	SelectedMarkerStyle = lipgloss.NewStyle().
				Foreground(lipgloss.Color("82"))

	UnselectedMarkerStyle = lipgloss.NewStyle().
				Foreground(lipgloss.Color("240"))

	CursorStyle = lipgloss.NewStyle().
			Background(lipgloss.Color("237"))

	SymlinkStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("180"))

	SizeStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("240"))

	// Details
	DetailLabelStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("75"))

	DetailValueStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("252"))

	// Status bar
	StatusBarStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("252")).
			Background(lipgloss.Color("236"))

	HelpKeyStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("75")).
			Background(lipgloss.Color("236"))

	HelpDescStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("240")).
			Background(lipgloss.Color("236"))

	// Title
	TitleStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("229")).
			Padding(0, 1)

	// Messages
	SuccessStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("82"))

	ErrorStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("196"))

	// Cumulative badge
	CumulativeBadge = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("229")).
			Background(lipgloss.Color("63")).
			Padding(0, 1)

	SingleLayerBadge = lipgloss.NewStyle().
				Bold(true).
				Foreground(lipgloss.Color("229")).
				Background(lipgloss.Color("240")).
				Padding(0, 1)
)
