package main

import (
	"flag"
	"fmt"
	"os"

	"imgchk/internal/image"
	"imgchk/internal/ui"

	tea "github.com/charmbracelet/bubbletea"
)

func main() {
	outputDir := flag.String("o", ".", "output directory for extracted files")
	flag.Usage = func() {
		fmt.Fprintf(os.Stderr, "Usage: imgchk [flags] <image.tar | image:tag>\n\nFlags:\n")
		flag.PrintDefaults()
	}
	flag.Parse()

	if flag.NArg() < 1 {
		flag.Usage()
		os.Exit(1)
	}

	source := flag.Arg(0)

	fmt.Printf("Loading image: %s\n", source)
	img, err := image.Load(source)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error loading image: %v\n", err)
		os.Exit(1)
	}

	fmt.Println("Analyzing layers...")
	info, err := image.Analyze(img)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error analyzing image: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Found %d layers, launching TUI...\n", len(info.Layers))

	app := ui.NewApp(info, *outputDir)
	p := tea.NewProgram(app, tea.WithAltScreen(), tea.WithMouseCellMotion())
	if _, err := p.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}
}
