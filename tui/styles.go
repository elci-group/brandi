package main

// Styles: Brandi's hot-pink control-room palette.

import "github.com/charmbracelet/lipgloss"

const (
	colorPrimary = "#FF2DAA"
	colorDark    = "#160812"
	colorLight   = "#FFF7FC"
	colorInk     = "#160812"
	colorMuted   = "#6F5B68"
	colorRed     = "#FF4D6D"
	colorAmber   = "#FFB020"
	colorCyan    = "#58D5FF"
)

var (
	styleHeader = lipgloss.NewStyle().
			Foreground(lipgloss.Color(colorPrimary)).
			Background(lipgloss.Color(colorDark)).
			Bold(true).
			Padding(0, 2)

	styleHeaderDim = lipgloss.NewStyle().
			Foreground(lipgloss.Color(colorLight)).
			Background(lipgloss.Color(colorDark)).
			Padding(0, 2)

	styleTabActive = lipgloss.NewStyle().
			Foreground(lipgloss.Color(colorInk)).
			Background(lipgloss.Color(colorPrimary)).
			Bold(true).
			Padding(0, 2)

	styleTabInactive = lipgloss.NewStyle().
				Foreground(lipgloss.Color(colorMuted)).
				Padding(0, 2)

	stylePanel = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color(colorPrimary)).
			Padding(0, 1)

	stylePanelTitle = lipgloss.NewStyle().
			Foreground(lipgloss.Color(colorPrimary)).
			Bold(true)

	styleMuted = lipgloss.NewStyle().Foreground(lipgloss.Color(colorMuted))
	styleBold  = lipgloss.NewStyle().Bold(true)

	styleErr  = lipgloss.NewStyle().Foreground(lipgloss.Color(colorRed)).Bold(true)
	styleWarn = lipgloss.NewStyle().Foreground(lipgloss.Color(colorAmber))
	styleInfo = lipgloss.NewStyle().Foreground(lipgloss.Color(colorCyan))
	stylePass = lipgloss.NewStyle().Foreground(lipgloss.Color(colorPrimary)).Bold(true)

	styleFooter = lipgloss.NewStyle().
			Foreground(lipgloss.Color(colorLight)).
			Background(lipgloss.Color(colorDark)).
			Padding(0, 1)
)

// scoreColor picks the score color: red < 50, amber < 80, brand pink >= 80.
func scoreColor(score float64) lipgloss.Color {
	switch {
	case score < 50:
		return lipgloss.Color(colorRed)
	case score < 80:
		return lipgloss.Color(colorAmber)
	default:
		return lipgloss.Color(colorPrimary)
	}
}

// severityGlyph maps a severity string to its styled marker.
func severityGlyph(sev string) string {
	switch sev {
	case "Error":
		return styleErr.Render("✗")
	case "Warning":
		return styleWarn.Render("⚠")
	default:
		return styleInfo.Render("ℹ")
	}
}

// bar renders a horizontal gauge of the given width for score/100.
func bar(score float64, width int) string {
	if width < 2 {
		return ""
	}
	filled := int(score / 100 * float64(width))
	if filled > width {
		filled = width
	}
	if filled < 0 {
		filled = 0
	}
	color := scoreColor(score)
	return lipgloss.NewStyle().Foreground(color).Render(repeat("█", filled)) +
		styleMuted.Render(repeat("░", width-filled))
}

func repeat(s string, n int) string {
	out := ""
	for i := 0; i < n; i++ {
		out += s
	}
	return out
}
