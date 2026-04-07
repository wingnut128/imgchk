package ui

import (
	"fmt"

	"imgchk/internal/extract"
	"imgchk/internal/image"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

// FocusedPane tracks which pane has keyboard focus.
type FocusedPane int

const (
	PaneLayerList FocusedPane = iota
	PaneFileTree
	PaneDetails
)

// Custom messages
type extractionCompleteMsg struct {
	count int
	err   error
}

type clearMessageMsg struct{}

// App is the root bubbletea model.
type App struct {
	imageInfo     *image.ImageInfo
	outputDir     string
	keys          KeyMap
	focused       FocusedPane
	width         int
	height        int
	layerList     LayerListModel
	fileTree      FileTreeModel
	details       DetailsModel
	statusBar     StatusBarModel
	cumulative    bool
	selectedFiles map[string]bool
	extracting    bool
	lastLayerIdx  int
}

// NewApp creates the root TUI model.
func NewApp(info *image.ImageInfo, outputDir string) App {
	selected := make(map[string]bool)

	a := App{
		imageInfo:     info,
		outputDir:     outputDir,
		keys:          DefaultKeyMap(),
		focused:       PaneLayerList,
		selectedFiles: selected,
		lastLayerIdx:  -1,
	}

	return a
}

// Init returns the initial command.
func (a App) Init() tea.Cmd {
	return nil
}

// Update handles all messages.
func (a App) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		a.width = msg.Width
		a.height = msg.Height
		a.recalcLayout()
		a.syncLayerSelection()
		return a, nil

	case extractionCompleteMsg:
		a.extracting = false
		if msg.err != nil {
			a.statusBar.SetMessage(ErrorStyle.Render(fmt.Sprintf("Extraction failed: %v", msg.err)))
		} else {
			a.statusBar.SetMessage(SuccessStyle.Render(fmt.Sprintf("Extracted %d files to %s", msg.count, a.outputDir)))
		}
		return a, nil

	case clearMessageMsg:
		a.statusBar.ClearMessage()
		return a, nil

	case tea.KeyMsg:
		// Global keys
		switch msg.String() {
		case "q", "ctrl+c":
			return a, tea.Quit
		case "tab":
			a.focused = (a.focused + 1) % 3
			return a, nil
		case "t":
			a.cumulative = !a.cumulative
			a.syncLayerSelection()
			return a, nil
		case "e":
			if len(a.selectedFiles) > 0 && !a.extracting {
				a.extracting = true
				paths := make([]string, 0, len(a.selectedFiles))
				for p := range a.selectedFiles {
					paths = append(paths, p)
				}
				a.statusBar.SetMessage("Extracting...")
				return a, extractCmd(a.imageInfo, paths, a.outputDir, a.cumulative, a.layerList.SelectedIndex())
			}
			return a, nil
		}

		// Delegate to focused pane
		var cmd tea.Cmd
		switch a.focused {
		case PaneLayerList:
			a.layerList, cmd = a.layerList.Update(msg)
			a.syncLayerSelection()
		case PaneFileTree:
			a.fileTree, cmd = a.fileTree.Update(msg)
		case PaneDetails:
			a.details, cmd = a.details.Update(msg)
		}

		a.statusBar.selectedCount = len(a.selectedFiles)
		return a, cmd
	}

	return a, nil
}

func (a *App) syncLayerSelection() {
	idx := a.layerList.SelectedIndex()
	if idx == a.lastLayerIdx && a.fileTree.tree != nil {
		// Only update cumulative state if needed
		if a.fileTree.cumulative != a.cumulative {
			a.updateFileTree(idx)
		}
		return
	}
	a.lastLayerIdx = idx
	a.updateFileTree(idx)
}

func (a *App) updateFileTree(idx int) {
	if idx < 0 || idx >= len(a.imageInfo.Layers) {
		return
	}

	var tree *image.FileTree
	if a.cumulative {
		trees := make([]*image.FileTree, idx+1)
		for i := 0; i <= idx; i++ {
			trees[i] = a.imageInfo.Layers[i].FileTree
		}
		tree = image.MergeTrees(trees...)
	} else {
		tree = a.imageInfo.Layers[idx].FileTree
	}

	a.fileTree.SetTree(tree, a.cumulative)
	a.details.SetLayer(a.imageInfo.Layers[idx])
}

func (a *App) recalcLayout() {
	// Left pane: 35% width
	leftWidth := a.width * 35 / 100
	if leftWidth < 20 {
		leftWidth = 20
	}
	rightWidth := a.width - leftWidth

	// Heights
	statusHeight := 1
	mainHeight := a.height - statusHeight
	fileTreeHeight := mainHeight * 60 / 100
	detailsHeight := mainHeight - fileTreeHeight

	a.layerList.SetSize(leftWidth-2, mainHeight-2)
	a.fileTree.SetSize(rightWidth-2, fileTreeHeight-2)
	a.details.SetSize(rightWidth-2, detailsHeight-2)
	a.statusBar.SetSize(a.width)
}

// View composes the full layout.
func (a App) View() string {
	if a.width == 0 || a.height == 0 {
		return "Loading..."
	}

	leftWidth := a.width * 35 / 100
	if leftWidth < 20 {
		leftWidth = 20
	}
	rightWidth := a.width - leftWidth

	statusHeight := 1
	mainHeight := a.height - statusHeight
	fileTreeHeight := mainHeight * 60 / 100
	detailsHeight := mainHeight - fileTreeHeight

	// Render panes with borders
	leftBorder := a.borderStyle(PaneLayerList).Width(leftWidth - 2).Height(mainHeight - 2)
	fileTreeBorder := a.borderStyle(PaneFileTree).Width(rightWidth - 2).Height(fileTreeHeight - 2)
	detailsBorder := a.borderStyle(PaneDetails).Width(rightWidth - 2).Height(detailsHeight - 2)

	leftPane := leftBorder.Render(a.layerList.View())
	fileTreePane := fileTreeBorder.Render(a.fileTree.View())
	detailsPane := detailsBorder.Render(a.details.View())

	rightPane := lipgloss.JoinVertical(lipgloss.Left, fileTreePane, detailsPane)
	mainArea := lipgloss.JoinHorizontal(lipgloss.Top, leftPane, rightPane)

	a.statusBar.selectedCount = len(a.selectedFiles)
	statusBar := a.statusBar.View()

	return lipgloss.JoinVertical(lipgloss.Left, mainArea, statusBar)
}

func (a App) borderStyle(pane FocusedPane) lipgloss.Style {
	if a.focused == pane {
		return ActiveBorderStyle
	}
	return InactiveBorderStyle
}

func extractCmd(info *image.ImageInfo, paths []string, outputDir string, cumulative bool, layerIdx int) tea.Cmd {
	return func() tea.Msg {
		var err error
		if cumulative {
			err = extract.ExtractFiles(info.Image, paths, outputDir)
		} else {
			if layerIdx >= 0 && layerIdx < len(info.Layers) {
				err = extract.ExtractFromLayer(info.Layers[layerIdx].Layer, paths, outputDir)
			}
		}
		return extractionCompleteMsg{count: len(paths), err: err}
	}
}
