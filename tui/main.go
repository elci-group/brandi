package main

// brandi-tui: a Charm (bubbletea/lipgloss) terminal UI for the brandi
// brand-coherence linter. It wraps the brandi binary — see README.md.

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	tea "github.com/charmbracelet/bubbletea"
)

func main() {
	path := flag.String("path", "", "project path (default: discover the nearest Brandi genome)")
	workspace := flag.String("workspace", "", "portfolio root (default: BRANDI_WORKSPACE_ROOT or the user's home)")
	brandi := flag.String("brandi", "", "path to the brandi binary (default: project release, debug, then PATH)")
	noWatch := flag.Bool("no-watch", false, "start with live re-lint disabled")
	startTab := flag.String("tab", "overview", "initial view: overview, findings, rules, social, tape, or growth")
	flag.Parse()

	*path = resolveProjectPath(*path)
	*workspace = resolveWorkspacePath(*workspace)
	b := resolveBrandi(*brandi, *path)
	if b == "" {
		fmt.Fprintln(os.Stderr, "brandi binary not found; install it or pass --brandi")
		os.Exit(2)
	}

	m := newModel(*path, *workspace, b, !*noWatch)
	m.tab = parseStartTab(*startTab)
	p := tea.NewProgram(m, tea.WithAltScreen())
	if _, err := p.Run(); err != nil {
		fmt.Fprintln(os.Stderr, "tui:", err)
		os.Exit(1)
	}
}

func parseStartTab(value string) tab {
	for index, name := range tabNames {
		if strings.EqualFold(value, name) || (value == "tape" && name == "Tape Studio") {
			return tab(index)
		}
	}
	return tabOverview
}

func resolveWorkspacePath(flagValue string) string {
	if flagValue == "" {
		flagValue = os.Getenv("BRANDI_WORKSPACE_ROOT")
	}
	if flagValue == "" {
		flagValue, _ = os.UserHomeDir()
	}
	if flagValue == "" {
		flagValue = "."
	}
	abs, err := filepath.Abs(flagValue)
	if err == nil {
		return abs
	}
	return flagValue
}

func resolveProjectPath(flagValue string) string {
	if flagValue == "" {
		flagValue = os.Getenv("BRANDI_PROJECT_ROOT")
	}
	if flagValue == "" {
		cwd, err := os.Getwd()
		if err == nil {
			flagValue = discoverProjectRoot(cwd)
		}
	}
	if flagValue == "" {
		flagValue = "."
	}
	abs, err := filepath.Abs(flagValue)
	if err == nil {
		return abs
	}
	return flagValue
}

func discoverProjectRoot(start string) string {
	current, err := filepath.Abs(start)
	if err != nil {
		return start
	}
	for {
		if hasGenome(current) {
			return current
		}
		parent := filepath.Dir(current)
		if parent == current {
			break
		}
		current = parent
	}
	// Running `brandi-tui` from a workspace home is common. Prefer its
	// canonical `brandi/` child when that child carries a valid genome.
	candidate := filepath.Join(start, "brandi")
	if hasGenome(candidate) {
		return candidate
	}
	return start
}

func hasGenome(path string) bool {
	info, err := os.Stat(filepath.Join(path, ".brandi", "identity.yaml"))
	return err == nil && !info.IsDir()
}

// resolveBrandi finds the binary: explicit flag, this project's builds,
// PATH, then legacy paths relative to the current directory.
func resolveBrandi(flagValue, projectPath string) string {
	if flagValue != "" {
		return flagValue
	}
	for _, cand := range []string{
		filepath.Join(projectPath, "target/release/brandi"),
		filepath.Join(projectPath, "target/debug/brandi"),
		"../target/release/brandi",
		"../target/debug/brandi",
		"target/release/brandi",
		"target/debug/brandi",
	} {
		if _, err := os.Stat(cand); err == nil {
			abs, err := filepath.Abs(cand)
			if err == nil {
				return abs
			}
			return cand
		}
	}
	if p, err := exec.LookPath("brandi"); err == nil {
		return p
	}
	return ""
}
