package main

// Rendering: header, tabs, the four tab views, and the footer.

import (
	"fmt"
	"path/filepath"
	"strings"
	"time"

	"github.com/charmbracelet/lipgloss"
)

func (m model) headerView() string {
	wordmark := styleHeader.Render("Brandi")
	path := styleHeaderDim.Render(m.path)

	daemon := styleHeaderDim.Render("daemon ○ off")
	if m.daemon.Running {
		dot := "●"
		if m.blink {
			dot = "◉"
		}
		daemon = styleHeader.Render(fmt.Sprintf("daemon %s pid %d", dot, m.daemon.PID))
	}

	watch := styleHeaderDim.Render("watch ○")
	if m.watch {
		watch = styleHeader.Render("watch ●")
	}

	left := lipgloss.JoinHorizontal(lipgloss.Top, wordmark, " ", path)
	right := lipgloss.JoinHorizontal(lipgloss.Top, daemon, " ", watch)
	gap := m.width - lipgloss.Width(left) - lipgloss.Width(right)
	if gap < 1 {
		gap = 1
	}
	return left + repeat(" ", gap) + right
}

func (m model) tabsView() string {
	var parts []string
	for i, name := range tabNames {
		if tab(i) == m.tab {
			parts = append(parts, styleTabActive.Render(name))
		} else {
			parts = append(parts, styleTabInactive.Render(name))
		}
	}
	return lipgloss.JoinHorizontal(lipgloss.Top, parts...)
}

func (m model) footerView() string {
	help := "tab/1-6 views · r lint · d daemon · w watch · f portfolio · c account · [/] select · D disconnect · W wifi · e goal · p preset · n draft · s save · m metrics · a/x decide · q quit"
	status := ""
	if m.loading {
		status = m.spinner.View() + " linting…"
	} else if m.err != nil {
		status = styleErr.Render(m.err.Error())
	} else if !m.lastLint.IsZero() {
		status = styleMuted.Render("last lint " + ago(m.lastLint))
	}
	left := styleFooter.Render(help)
	right := styleFooter.Render(status)
	gap := m.width - lipgloss.Width(left) - lipgloss.Width(right)
	if gap < 1 {
		gap = 1
	}
	return left + repeat(" ", gap) + right
}

// --- Overview tab ---

func (m model) overviewView() string {
	content := m.overviewContent() + "\n" + m.portfolioView()
	viewport := m.vpOverview
	viewport.SetContent(content)
	return viewport.View()
}

// pendingReportPlaceholder is what any report-driven tab shows before the
// first lint has completed: a spinner while it's in flight, or an error
// with a retry hint if it failed. Findings and Rules used to render a
// blank panel in this state instead of sharing Overview's placeholder.
func (m model) pendingReportPlaceholder() string {
	if m.err != nil {
		return "\n  " + styleErr.Render("✗ "+m.err.Error()) +
			"\n  " + styleMuted.Render("press r to retry")
	}
	return "\n  " + m.spinner.View() + styleMuted.Render(" running brandi lint…")
}

func (m model) overviewContent() string {
	if m.report == nil {
		return m.pendingReportPlaceholder()
	}

	score := m.displayed
	scoreTxt := lipgloss.NewStyle().
		Foreground(scoreColor(score)).
		Bold(true).
		Render(fmt.Sprintf("%3.0f", score)) + styleMuted.Render("/100")

	gauge := m.gauge.ViewAs(score / 100)
	counts := fmt.Sprintf("%s %s  %s %s  %s %s",
		styleErr.Render("✗"), styleBold.Render(fmt.Sprint(m.report.Scores.Errors)),
		styleWarn.Render("⚠"), styleBold.Render(fmt.Sprint(m.report.Scores.Warnings)),
		styleInfo.Render("ℹ"), styleBold.Render(fmt.Sprint(m.report.Scores.Infos)))

	scorePanel := stylePanelTitle.Render("Brand coherence") + "\n\n" +
		lipgloss.JoinHorizontal(lipgloss.Top, scoreTxt+"  ", gauge) + "\n" + counts

	// Per-kind bars.
	kindOrder := []string{"repo_doc", "doc", "ui_string", "style"}
	var kindLines []string
	for _, k := range kindOrder {
		s, ok := m.report.Scores.ByKind[k]
		if !ok {
			continue
		}
		kindLines = append(kindLines,
			fmt.Sprintf("%-10s %s %s",
				k,
				bar(float64(s), 24),
				lipgloss.NewStyle().Foreground(scoreColor(float64(s))).Render(fmt.Sprintf("%3d", s))))
	}
	kindPanel := stylePanelTitle.Render("By surface") + "\n\n" + strings.Join(kindLines, "\n")

	top := lipgloss.JoinHorizontal(lipgloss.Top,
		stylePanel.Render(scorePanel), "  ",
		stylePanel.Render(kindPanel))

	// History sparkline + surfaces.
	var bottomLeft string
	if len(m.history) > 0 {
		var scores []int
		start := len(m.history) - 40
		if start < 0 {
			start = 0
		}
		for _, e := range m.history[start:] {
			scores = append(scores, e.Score)
		}
		last := m.history[len(m.history)-1]
		bottomLeft = stylePanelTitle.Render("Score history") + "\n\n" +
			lipgloss.NewStyle().Foreground(scoreColor(float64(last.Score))).Render(sparkline(scores)) +
			"\n" + styleMuted.Render(fmt.Sprintf("%d daemon lints, latest %d/100", len(m.history), last.Score))
	} else {
		bottomLeft = stylePanelTitle.Render("Score history") + "\n\n" +
			styleMuted.Render("no daemon history yet — press d to start the daemon")
	}

	var surfaceLines []string
	for _, k := range kindOrder {
		if n, ok := m.report.SurfaceCounts[k]; ok {
			surfaceLines = append(surfaceLines, fmt.Sprintf("%-10s %d", k, n))
		}
	}
	bottomRight := stylePanelTitle.Render("Surfaces scanned") + "\n\n" + strings.Join(surfaceLines, "\n")

	bottom := lipgloss.JoinHorizontal(lipgloss.Top,
		stylePanel.Render(bottomLeft), "  ",
		stylePanel.Render(bottomRight))

	return top + "\n" + bottom
}

func (m model) portfolioView() string {
	title := stylePanelTitle.Render("Project portfolio") + "  " + styleMuted.Render(m.workspace)
	if !m.projectsOK {
		return stylePanel.Render(title + "\n\n" + m.spinner.View() + styleMuted.Render(" discovering projects and rating brand health…"))
	}
	if len(m.projects) == 0 {
		return stylePanel.Render(title + "\n\n" + styleMuted.Render("No project markers found. Press f to rescan."))
	}
	rows := []string{styleMuted.Render("RATING       STATUS                PROJECT · EVIDENCE")}
	for _, project := range m.projects {
		relative, err := filepath.Rel(m.workspace, project.Path)
		if err != nil || strings.HasPrefix(relative, "..") {
			relative = project.Path
		}
		statusStyle := styleMuted
		if project.Configured {
			statusStyle = lipgloss.NewStyle().Foreground(scoreColor(float64(project.Score)))
		}
		// Truncate before styling: ANSI codes from .Render() would
		// otherwise get counted (and possibly split) by a naive rune
		// truncation applied afterward.
		rows = append(rows, fmt.Sprintf(
			"%-12s %-20s %s  %s",
			truncate(project.Rating, 12),
			statusStyle.Render(truncate(project.Status, 20)),
			styleBold.Render(truncate(relative, 40)),
			styleMuted.Render(truncate(project.Detail, 40)),
		))
	}
	return stylePanel.Render(title + "\n\n" + strings.Join(rows, "\n"))
}

// --- Findings tab ---

func renderFindings(r *Report) string {
	if len(r.Findings) == 0 {
		return stylePass.Render("  ✓ no findings — every surface is on-brand")
	}
	var b strings.Builder
	lastPath := ""
	for _, f := range r.Findings {
		if f.Path != lastPath {
			if lastPath != "" {
				b.WriteString("\n")
			}
			b.WriteString(styleBold.Render(f.Path) + "\n")
			lastPath = f.Path
		}
		line := ""
		if f.Line != nil {
			line = fmt.Sprintf("L%d ", *f.Line)
		}
		b.WriteString(fmt.Sprintf("  %s %s[%s] %s\n",
			severityGlyph(f.Severity), line, f.RuleID, f.Message))
		if f.Suggestion != nil && *f.Suggestion != "" {
			b.WriteString(styleMuted.Render("      → "+*f.Suggestion) + "\n")
		}
	}
	return b.String()
}

func (m model) findingsView() string {
	if m.report == nil {
		return m.pendingReportPlaceholder()
	}
	return m.vp.View()
}

// --- Rules tab ---

func (m model) rulesView() string {
	if m.report == nil {
		return m.pendingReportPlaceholder()
	}
	counts := map[string]int{}
	worst := map[string]string{}
	rank := map[string]int{"Info": 1, "Warning": 2, "Error": 3}
	for _, f := range m.report.Findings {
		counts[f.RuleID]++
		if rank[f.Severity] > rank[worst[f.RuleID]] {
			worst[f.RuleID] = f.Severity
		}
	}
	var lines []string
	for _, rule := range ruleCatalog {
		n := counts[rule.ID]
		if n == 0 {
			lines = append(lines, fmt.Sprintf("  %s %s %s",
				stylePass.Render("✓"),
				styleBold.Render(rule.ID),
				styleMuted.Render("— "+rule.Desc)))
		} else {
			lines = append(lines, fmt.Sprintf("  %s %s %s %s",
				severityGlyph(worst[rule.ID]),
				styleBold.Render(rule.ID),
				styleMuted.Render("— "+rule.Desc),
				styleWarn.Render(fmt.Sprintf("(%d)", n))))
		}
	}
	header := stylePanelTitle.Render(fmt.Sprintf(
		"Rule catalog — %d rules, %d passing", len(ruleCatalog), len(ruleCatalog)-len(counts)))
	return header + "\n\n" + strings.Join(lines, "\n")
}

// --- Social tab ---

func (m model) socialView() string {
	if m.accountWizard {
		return m.accountWizardView()
	}
	if m.wifiWizard {
		return m.wifiWizardView()
	}
	header := stylePanelTitle.Render("Social accounts") + "  " +
		styleMuted.Render("project-linked sources and surfaces · c signs in · [/] selects · D twice disconnects · W pairs Android") + "\n\n"
	status := ""
	if m.accountStatus != "" {
		status = stylePass.Render("✓ "+m.accountStatus) + "\n\n"
	}
	if m.wifiStatus != "" {
		status += stylePass.Render("✓ "+m.wifiStatus) + "\n\n"
	}
	if !m.socialOK {
		return header + status + "  " + m.spinner.View() + styleMuted.Render(" loading content graph…")
	}
	return header + status + m.socialAccountsView() + "\n\n" + m.vpSocial.View()
}

func (m model) socialAccountsView() string {
	if len(m.socialAccounts) == 0 {
		return stylePanel.Render(styleMuted.Render("No accounts linked. Press c to sign in securely."))
	}
	rows := []string{styleBold.Render("   provider     identity                     credential        roles")}
	for index, account := range m.socialAccounts {
		cursor := " "
		if index == m.socialAccountIndex {
			cursor = stylePass.Render("›")
		}
		credentialState := "missing"
		if account.CredentialAvailable {
			credentialState = "ready"
		}
		credential := truncate(account.CredentialEnv+":"+credentialState, 25)
		if account.CredentialAvailable {
			credential = stylePass.Render(credential)
		} else {
			credential = styleErr.Render(credential)
		}
		identity := fmt.Sprintf("%s (@%s)", account.DisplayName, account.Handle)
		rows = append(rows, fmt.Sprintf("%s  %-12s %-28s %-17s %s",
			cursor,
			truncate(account.Provider, 12),
			truncate(identity, 28),
			credential,
			styleMuted.Render("source · surface"),
		))
		if index == m.socialAccountIndex {
			rows = append(rows,
				styleMuted.Render("      source  content + metrics evidence")+"\n"+
					styleMuted.Render("      surface public profile + publishing identity")+"\n"+
					styleMuted.Render("      "+account.ProfileURL),
			)
		}
	}
	return stylePanel.Render(strings.Join(rows, "\n"))
}

func (m model) accountWizardView() string {
	steps := []string{"provider", "handle", "display name", "profile URL", "credential env", "session token", "public bio"}
	for index := range steps {
		label := fmt.Sprintf("%d %s", index+1, steps[index])
		if index == m.accountStep {
			steps[index] = stylePass.Render(label)
		} else {
			steps[index] = styleMuted.Render(label)
		}
	}
	provider := socialProviders[m.accountProvider].Name
	body := stylePanelTitle.Render("Social account sign-in") + "  " +
		styleMuted.Render("provider credential → project source + governed surface") + "\n\n" +
		styleWarn.Render("Tokens remain in this TUI session only: masked, never stored, and never passed in argv.") + "\n" +
		styleMuted.Render("Leave the token blank when the credential environment variable was exported before launch.") + "\n\n" +
		strings.Join(steps, "  ") + "\n\n" +
		"Provider        " + styleBold.Render("‹ "+provider+" ›") + styleMuted.Render("  left/right changes") + "\n" +
		"Handle          " + m.accountHandle.View() + "\n" +
		"Display name    " + m.accountDisplayName.View() + "\n" +
		"Profile URL     " + m.accountProfileURL.View() + "\n" +
		"Credential env  " + m.accountCredential.View() + "\n" +
		"Session token   " + m.accountToken.View() + "\n" +
		"Public bio      " + m.accountBio.View() + "\n\n" +
		styleMuted.Render("tab/shift-tab moves · enter advances/signs in · esc cancels")
	if m.loading {
		body += "\n\n" + m.spinner.View() + styleMuted.Render(" linking account…")
	} else if m.accountStatus != "" {
		body += "\n\n" + styleErr.Render(m.accountStatus)
	}
	return stylePanel.Render(body)
}

func (m model) wifiWizardView() string {
	steps := []string{"1 Pair endpoint", "2 Pairing code", "3 Debug endpoint"}
	for index := range steps {
		if index == m.wifiStep {
			steps[index] = stylePass.Render(steps[index])
		} else {
			steps[index] = styleMuted.Render(steps[index])
		}
	}

	instructions := stylePanelTitle.Render("Wi-Fi connection wizard") + "  " +
		styleMuted.Render("Android 11+ secure pairing") + "\n\n" +
		"On Android, open " + styleBold.Render("Settings → Developer options → Wireless debugging") + ".\n" +
		"Choose " + styleBold.Render("Pair device with pairing code") + ", then enter both addresses shown by Android.\n" +
		styleWarn.Render("The pairing endpoint and debug endpoint often use different ports.") + "\n\n" +
		strings.Join(steps, "   ") + "\n\n" +
		"Pair endpoint   " + m.wifiPair.View() + "\n" +
		"Pairing code    " + m.wifiCode.View() + "  " + styleMuted.Render("masked; never passed on the command line") + "\n" +
		"Debug endpoint  " + m.wifiConnect.View() + "\n\n" +
		styleMuted.Render("tab/shift-tab moves · enter advances or pairs+connects · esc cancels")

	if m.loading {
		instructions += "\n\n" + m.spinner.View() + styleMuted.Render(" pairing and connecting…")
	} else if m.wifiStatus != "" {
		instructions += "\n\n" + styleErr.Render(m.wifiStatus)
	}
	return stylePanel.Render(instructions)
}

var tapePresets = []string{"product-tour", "lint-failure-fix", "social-plan"}

func renderTapeDraft(draft *TapeDraft) string {
	if draft == nil {
		return ""
	}
	var b strings.Builder
	b.WriteString(stylePanelTitle.Render(draft.Plan.Title) + "\n")
	b.WriteString(styleMuted.Render("provider: "+draft.Provider) + "\n\n")
	for i, scene := range draft.Plan.Scenes {
		b.WriteString(fmt.Sprintf("%s %s\n", stylePass.Render(fmt.Sprintf("%02d", i+1)), styleBold.Render(scene.Title)))
		command := append([]string{scene.Command.Program}, scene.Command.Args...)
		b.WriteString("   " + strings.Join(command, " ") + "\n")
		b.WriteString(styleMuted.Render(fmt.Sprintf("   %dms · %s", scene.PauseMS, scene.Rationale)) + "\n\n")
	}
	if len(draft.Critique) > 0 {
		b.WriteString(stylePanelTitle.Render("Reasoning critique") + "\n")
		for _, issue := range draft.Critique {
			b.WriteString(fmt.Sprintf("  • [%s] %s\n", issue.Code, issue.Message))
		}
	}
	b.WriteString("\n" + stylePanelTitle.Render("Compiled tape") + "\n" + draft.Tape)
	return b.String()
}

func (m model) tapeView() string {
	header := stylePanelTitle.Render("Tape Studio") + "  " + styleMuted.Render("structured reasoning → deterministic VHS") + "\n\n"
	form := "Goal    " + m.tapeGoal.View() + "\n" +
		"Preset  " + styleBold.Render(tapePresets[m.tapePreset]) + "  " + styleMuted.Render("p cycles · e edits · n drafts · s validates+saves")
	status := ""
	if m.tapeSaved {
		status = "\n" + stylePass.Render("✓ saved demos/brandi-demo.tape after validation")
	}
	if m.tapeDraft == nil {
		return header + stylePanel.Render(form+status) + "\n\n" + styleMuted.Render("Press n to generate the first draft.")
	}
	return header + stylePanel.Render(form+status) + "\n" + m.vpTape.View()
}

func (m model) growthView() string {
	header := stylePanelTitle.Render("Growth control room") + "  " + styleMuted.Render("Padagonia plans · Git/GitHub signals · approval queue") + "\n\n"
	if !m.growthOK {
		return header + m.spinner.View() + styleMuted.Render(" collecting evidence…")
	}
	if m.growthConfirm != nil {
		action := "REJECT"
		if m.growthApprove {
			action = "APPROVE"
		}
		body := fmt.Sprintf(
			"%s exact revision\n\nID: %s\nRevision: %s\nChannel: %s\nActor: current credential-bound local principal\nContent:\n%s\n\nConfirmation  %s",
			action, m.growthConfirm.ID, m.growthConfirm.RevisionHash,
			m.growthConfirm.Channel, m.growthConfirm.Content, m.growthConfirmation.View(),
		)
		if m.growthApprove {
			body += "\nDestination   " + m.growthDestination.View()
		}
		body += "\n\n" + styleMuted.Render("Enter submits · Tab changes field · Esc cancels")
		return header + stylePanel.Render(body)
	}
	pending := 0
	for _, draft := range m.growthQueue {
		if draft.Status == "pending" {
			pending++
		}
	}
	controls := styleMuted.Render(fmt.Sprintf("%d pending · a/x opens a second exact-confirmation review", pending)) + "\n\n"
	return header + controls + m.vpGrowth.View()
}

// truncate shortens s to at most width runes, replacing the tail with an
// ellipsis when it doesn't fit. Fixed-width columns (portfolioView) would
// otherwise overflow and break alignment on a long project path or status
// string instead of just clipping it.
func truncate(s string, width int) string {
	r := []rune(s)
	if len(r) <= width {
		return s
	}
	if width <= 1 {
		return string(r[:width])
	}
	return string(r[:width-1]) + "…"
}

func ago(t time.Time) string {
	d := time.Since(t)
	switch {
	case d < 2*time.Second:
		return "just now"
	case d < time.Minute:
		return fmt.Sprintf("%ds ago", int(d.Seconds()))
	case d < time.Hour:
		return fmt.Sprintf("%dm ago", int(d.Minutes()))
	default:
		return fmt.Sprintf("%dh ago", int(d.Hours()))
	}
}
