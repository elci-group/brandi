package main

// The bubbletea model: state, messages, and the update loop.
//
// Dynamism comes from three recurring commands:
//   - animTick (~90ms): score count-up easing, daemon badge pulse, gauge frames
//   - watchTick (2.5s): live re-lint while watch mode is on
//   - the bubbles spinner's own tick chain while a lint runs

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/charmbracelet/bubbles/progress"
	"github.com/charmbracelet/bubbles/spinner"
	"github.com/charmbracelet/bubbles/textinput"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
)

type tab int

const (
	tabOverview tab = iota
	tabFindings
	tabRules
	tabSocial
	tabTape
	tabGrowth
)

var tabNames = []string{"Overview", "Findings", "Rules", "Social", "Tape Studio", "Growth"}

var socialProviders = []struct {
	Name          string
	CredentialEnv string
}{
	{"mastodon", "MASTODON_ACCESS_TOKEN"},
	{"bluesky", "BLUESKY_APP_PASSWORD"},
	{"x", "X_ACCESS_TOKEN"},
	{"instagram", "META_ACCESS_TOKEN"},
	{"linkedin", "LINKEDIN_ACCESS_TOKEN"},
	{"youtube", "YOUTUBE_ACCESS_TOKEN"},
	{"tiktok", "TIKTOK_ACCESS_TOKEN"},
}

func socialProviderCredential(index int) string {
	return socialProviders[index%len(socialProviders)].CredentialEnv
}

// Messages.
type animTickMsg time.Time
type watchTickMsg time.Time

type lintResultMsg struct {
	report  *Report
	history []HistoryEntry
	daemon  DaemonState
	err     error
}
type daemonResultMsg struct{ err error }
type socialResultMsg struct {
	text     string
	accounts []SocialAccount
	err      error
}
type accountResultMsg struct {
	accounts []SocialAccount
	status   string
	err      error
}
type tapeResultMsg struct {
	draft *TapeDraft
	saved bool
	err   error
}
type growthResultMsg struct {
	text  string
	queue []ContentDraft
	err   error
}
type growthDecisionMsg struct{ err error }
type projectResultMsg struct {
	projects []ProjectSummary
	err      error
}
type wifiResultMsg struct {
	text string
	err  error
}

func animTick() tea.Cmd {
	return tea.Tick(90*time.Millisecond, func(t time.Time) tea.Msg { return animTickMsg(t) })
}

func watchTick() tea.Cmd {
	return tea.Tick(2500*time.Millisecond, func(t time.Time) tea.Msg { return watchTickMsg(t) })
}

func runLintCmd(brandi, path string) tea.Cmd {
	return func() tea.Msg {
		report, err := RunLint(brandi, path)
		return lintResultMsg{
			report:  report,
			history: LoadHistory(path),
			daemon:  DaemonStatus(path),
			err:     err,
		}
	}
}

func toggleDaemonCmd(brandi, path string, running bool) tea.Cmd {
	return func() tea.Msg {
		return daemonResultMsg{err: ToggleDaemon(brandi, path, running)}
	}
}

func socialCmd(brandi, path string) tea.Cmd {
	return func() tea.Msg {
		text, err := SocialGraph(brandi, path)
		if err != nil {
			return socialResultMsg{err: err}
		}
		accounts, err := LoadSocialAccounts(brandi, path)
		return socialResultMsg{text: text, accounts: accounts, err: err}
	}
}

func accountConnectCmd(brandi, path string, input SocialAccountInput) tea.Cmd {
	return func() tea.Msg {
		accounts, err := ConnectSocialAccount(brandi, path, input)
		return accountResultMsg{accounts: accounts, status: "account linked as source and surface", err: err}
	}
}

func accountDisconnectCmd(brandi, path, id string) tea.Cmd {
	return func() tea.Msg {
		accounts, err := DisconnectSocialAccount(brandi, path, id)
		return accountResultMsg{accounts: accounts, status: "project account link removed", err: err}
	}
}

func tapeCmd(brandi, path, goal, preset string, save bool) tea.Cmd {
	return func() tea.Msg {
		draft, err := GenerateTape(brandi, path, goal, preset, save)
		return tapeResultMsg{draft: draft, saved: save, err: err}
	}
}

func growthCmd(brandi, path string) tea.Cmd {
	return func() tea.Msg {
		value, queue, err := GrowthSummary(brandi, path)
		return growthResultMsg{text: value, queue: queue, err: err}
	}
}

func growthDecisionCmd(brandi, path, id, confirmation, destination string, approve bool) tea.Cmd {
	return func() tea.Msg {
		return growthDecisionMsg{err: DecideGrowthDraft(brandi, path, id, confirmation, destination, approve)}
	}
}

func projectCmd(brandi, workspace string) tea.Cmd {
	return func() tea.Msg {
		projects, err := DiscoverProjects(brandi, workspace)
		return projectResultMsg{projects: projects, err: err}
	}
}

func wifiPairCmd(brandi, pairEndpoint, connectEndpoint, code string) tea.Cmd {
	return func() tea.Msg {
		cmd := exec.Command(
			brandi,
			"social", "adb", "wifi", "pair",
			"--pair-endpoint", pairEndpoint,
			"--connect-endpoint", connectEndpoint,
		)
		cmd.Env = append(os.Environ(), "BRANDI_ADB_PAIR_CODE="+code)
		out, err := cmd.CombinedOutput()
		if err != nil {
			return wifiResultMsg{err: fmt.Errorf("Wi-Fi pairing: %s", strings.TrimSpace(string(out)))}
		}
		return wifiResultMsg{text: strings.TrimSpace(string(out))}
	}
}

type model struct {
	path      string
	workspace string
	brandi    string
	watch     bool
	tab       tab
	width     int
	height    int

	report             *Report
	history            []HistoryEntry
	daemon             DaemonState
	social             string
	socialOK           bool
	socialAccounts     []SocialAccount
	socialAccountIndex int
	accountWizard      bool
	accountStep        int
	accountProvider    int
	accountHandle      textinput.Model
	accountDisplayName textinput.Model
	accountProfileURL  textinput.Model
	accountCredential  textinput.Model
	accountToken       textinput.Model
	accountBio         textinput.Model
	accountStatus      string
	disconnectArmed    string
	tapeDraft          *TapeDraft
	tapePreset         int
	tapeGoal           textinput.Model
	editingGoal        bool
	tapeSaved          bool
	growth             string
	growthQueue        []ContentDraft
	growthOK           bool
	growthConfirm      *ContentDraft
	growthApprove      bool
	growthConfirmStep  int
	growthConfirmation textinput.Model
	growthDestination  textinput.Model
	projects           []ProjectSummary
	projectsOK         bool
	wifiWizard         bool
	wifiStep           int
	wifiPair           textinput.Model
	wifiCode           textinput.Model
	wifiConnect        textinput.Model
	wifiStatus         string
	err                error

	loading  bool
	lastLint time.Time

	spinner    spinner.Model
	gauge      progress.Model
	vp         viewport.Model // findings
	vpOverview viewport.Model
	vpSocial   viewport.Model
	vpTape     viewport.Model
	vpGrowth   viewport.Model

	displayed float64 // animated score value
	target    float64
	blink     bool
	ticks     int
}

func newModel(path, workspace, brandi string, watch bool) model {
	s := spinner.New(
		spinner.WithSpinner(spinner.Dot),
		spinner.WithStyle(stylePass),
	)
	g := progress.New(
		progress.WithGradient(colorDark, colorPrimary),
		progress.WithScaledGradient(colorDark, colorPrimary),
		progress.WithoutPercentage(),
	)
	goal := textinput.New()
	goal.Placeholder = "What should this demo prove?"
	goal.SetValue("Show how Brandi prevents identity drift")
	goal.CharLimit = 180
	goal.Width = 64
	pair := textinput.New()
	pair.Placeholder = "192.168.1.50:37123 (pairing endpoint)"
	pair.CharLimit = 128
	pair.Width = 54
	code := textinput.New()
	code.Placeholder = "six-digit pairing code"
	code.CharLimit = 6
	code.Width = 24
	code.EchoMode = textinput.EchoPassword
	code.EchoCharacter = '•'
	connect := textinput.New()
	connect.Placeholder = "192.168.1.50:42131 (debug endpoint)"
	connect.CharLimit = 128
	connect.Width = 54
	growthConfirmation := textinput.New()
	growthConfirmation.Placeholder = "paste the exact draft ID or revision hash"
	growthConfirmation.CharLimit = 128
	growthConfirmation.Width = 72
	growthDestination := textinput.New()
	growthDestination.Placeholder = "exact destination, e.g. telegram:-100123 or adb:x"
	growthDestination.CharLimit = 180
	growthDestination.Width = 72
	accountHandle := textinput.New()
	accountHandle.Placeholder = "account handle"
	accountHandle.CharLimit = 120
	accountHandle.Width = 44
	accountDisplayName := textinput.New()
	accountDisplayName.Placeholder = "public display name"
	accountDisplayName.CharLimit = 120
	accountDisplayName.Width = 44
	accountProfileURL := textinput.New()
	accountProfileURL.Placeholder = "https://provider.example/profile"
	accountProfileURL.CharLimit = 300
	accountProfileURL.Width = 64
	accountCredential := textinput.New()
	accountCredential.SetValue(socialProviderCredential(0))
	accountCredential.CharLimit = 100
	accountCredential.Width = 36
	accountToken := textinput.New()
	accountToken.Placeholder = "provider token (session only; optional if env is set)"
	accountToken.CharLimit = 4096
	accountToken.Width = 54
	accountToken.EchoMode = textinput.EchoPassword
	accountToken.EchoCharacter = '•'
	accountBio := textinput.New()
	accountBio.Placeholder = "public profile bio"
	accountBio.CharLimit = 500
	accountBio.Width = 64
	return model{
		path:               path,
		workspace:          workspace,
		brandi:             brandi,
		watch:              watch,
		spinner:            s,
		gauge:              g,
		vp:                 viewport.New(80, 20),
		vpOverview:         viewport.New(80, 20),
		vpSocial:           viewport.New(80, 20),
		vpTape:             viewport.New(80, 20),
		vpGrowth:           viewport.New(80, 20),
		tapeGoal:           goal,
		wifiPair:           pair,
		wifiCode:           code,
		wifiConnect:        connect,
		growthConfirmation: growthConfirmation,
		growthDestination:  growthDestination,
		accountHandle:      accountHandle,
		accountDisplayName: accountDisplayName,
		accountProfileURL:  accountProfileURL,
		accountCredential:  accountCredential,
		accountToken:       accountToken,
		accountBio:         accountBio,
	}
}

func (m model) Init() tea.Cmd {
	return tea.Batch(
		runLintCmd(m.brandi, m.path),
		socialCmd(m.brandi, m.path),
		growthCmd(m.brandi, m.path),
		projectCmd(m.brandi, m.workspace),
		animTick(),
		watchTick(),
		m.spinner.Tick,
	)
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmds []tea.Cmd

	// The spinner chain sustains itself once started.
	var spinCmd tea.Cmd
	m.spinner, spinCmd = m.spinner.Update(msg)
	cmds = append(cmds, spinCmd)

	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width, m.height = msg.Width, msg.Height
		contentH := m.height - 6 // header + tabs + footer
		if contentH < 4 {
			contentH = 4
		}
		m.vp.Width, m.vp.Height = m.width-4, contentH
		m.vpOverview.Width, m.vpOverview.Height = m.width-4, contentH
		m.vpSocial.Width, m.vpSocial.Height = m.width-4, contentH
		m.vpTape.Width, m.vpTape.Height = m.width-4, contentH-5
		m.vpGrowth.Width, m.vpGrowth.Height = m.width-4, contentH
		m.gauge.Width = m.width - 24
		if m.gauge.Width < 10 {
			m.gauge.Width = 10
		}
		m.refreshViewport()

	case tea.KeyMsg:
		if m.growthConfirm != nil {
			switch msg.String() {
			case "esc":
				m.growthConfirm = nil
				m.growthConfirmation.Blur()
				m.growthDestination.Blur()
			case "tab", "shift+tab", "up", "down":
				if m.growthApprove {
					m.growthConfirmStep = (m.growthConfirmStep + 1) % 2
					if m.growthConfirmStep == 0 {
						m.growthDestination.Blur()
						m.growthConfirmation.Focus()
					} else {
						m.growthConfirmation.Blur()
						m.growthDestination.Focus()
					}
				}
			case "enter":
				if m.growthConfirmation.Value() != "" && (!m.growthApprove || m.growthDestination.Value() != "") {
					m.loading = true
					cmds = append(cmds, growthDecisionCmd(
						m.brandi, m.path, m.growthConfirm.ID,
						m.growthConfirmation.Value(), m.growthDestination.Value(), m.growthApprove,
					))
					m.growthConfirm = nil
					m.growthConfirmation.Blur()
					m.growthDestination.Blur()
				}
			default:
				var cmd tea.Cmd
				if m.growthApprove && m.growthConfirmStep == 1 {
					m.growthDestination, cmd = m.growthDestination.Update(msg)
				} else {
					m.growthConfirmation, cmd = m.growthConfirmation.Update(msg)
				}
				cmds = append(cmds, cmd)
			}
			return m, tea.Batch(cmds...)
		}
		if m.wifiWizard {
			switch msg.String() {
			case "esc":
				m.wifiWizard = false
				m.blurWifiInputs()
			case "tab", "down":
				m.wifiStep = (m.wifiStep + 1) % 3
				m.focusWifiInput()
			case "shift+tab", "up":
				m.wifiStep = (m.wifiStep + 2) % 3
				m.focusWifiInput()
			case "enter":
				if m.wifiStep < 2 {
					m.wifiStep++
					m.focusWifiInput()
				} else if m.wifiPair.Value() != "" && m.wifiCode.Value() != "" && m.wifiConnect.Value() != "" {
					m.loading = true
					m.blurWifiInputs()
					cmds = append(cmds, wifiPairCmd(m.brandi, m.wifiPair.Value(), m.wifiConnect.Value(), m.wifiCode.Value()))
				}
			default:
				var cmd tea.Cmd
				switch m.wifiStep {
				case 0:
					m.wifiPair, cmd = m.wifiPair.Update(msg)
				case 1:
					m.wifiCode, cmd = m.wifiCode.Update(msg)
				default:
					m.wifiConnect, cmd = m.wifiConnect.Update(msg)
				}
				cmds = append(cmds, cmd)
			}
			return m, tea.Batch(cmds...)
		}
		if m.accountWizard {
			switch msg.String() {
			case "esc":
				m.accountWizard = false
				m.blurAccountInputs()
				m.accountToken.SetValue("")
			case "tab", "down":
				m.accountStep = (m.accountStep + 1) % 7
				m.focusAccountInput()
			case "shift+tab", "up":
				m.accountStep = (m.accountStep + 6) % 7
				m.focusAccountInput()
			case "left", "right":
				if m.accountStep == 0 {
					delta := 1
					if msg.String() == "left" {
						delta = len(socialProviders) - 1
					}
					m.accountProvider = (m.accountProvider + delta) % len(socialProviders)
					m.accountCredential.SetValue(socialProviderCredential(m.accountProvider))
				}
			case "enter":
				if m.accountStep < 6 {
					m.accountStep++
					m.focusAccountInput()
				} else if m.accountHandle.Value() != "" && m.accountDisplayName.Value() != "" && m.accountProfileURL.Value() != "" && m.accountCredential.Value() != "" {
					m.loading = true
					m.blurAccountInputs()
					cmds = append(cmds, accountConnectCmd(m.brandi, m.path, SocialAccountInput{
						Provider: socialProviders[m.accountProvider].Name, Handle: m.accountHandle.Value(),
						DisplayName: m.accountDisplayName.Value(), ProfileURL: m.accountProfileURL.Value(),
						Bio: m.accountBio.Value(), CredentialEnv: m.accountCredential.Value(), Token: m.accountToken.Value(),
					}))
				}
			default:
				var cmd tea.Cmd
				switch m.accountStep {
				case 1:
					m.accountHandle, cmd = m.accountHandle.Update(msg)
				case 2:
					m.accountDisplayName, cmd = m.accountDisplayName.Update(msg)
				case 3:
					m.accountProfileURL, cmd = m.accountProfileURL.Update(msg)
				case 4:
					m.accountCredential, cmd = m.accountCredential.Update(msg)
				case 5:
					m.accountToken, cmd = m.accountToken.Update(msg)
				case 6:
					m.accountBio, cmd = m.accountBio.Update(msg)
				}
				cmds = append(cmds, cmd)
			}
			return m, tea.Batch(cmds...)
		}
		if m.editingGoal {
			if msg.String() == "esc" || msg.String() == "enter" {
				m.editingGoal = false
				m.tapeGoal.Blur()
			} else {
				var cmd tea.Cmd
				m.tapeGoal, cmd = m.tapeGoal.Update(msg)
				cmds = append(cmds, cmd)
			}
			return m, tea.Batch(cmds...)
		}
		switch msg.String() {
		case "q", "ctrl+c", "esc":
			return m, tea.Quit
		case "tab", "right", "l":
			m.tab = (m.tab + 1) % tab(len(tabNames))
		case "shift+tab", "left", "h":
			m.tab = (m.tab + tab(len(tabNames)) - 1) % tab(len(tabNames))
		case "1", "2", "3", "4", "5", "6":
			m.tab = tab(int(msg.String()[0] - '1'))
		case "e":
			if m.tab == tabTape {
				m.editingGoal = true
				m.tapeGoal.Focus()
				cmds = append(cmds, textinput.Blink)
			}
		case "p":
			if m.tab == tabTape {
				m.tapePreset = (m.tapePreset + 1) % 3
			}
		case "n":
			if m.tab == tabTape && !m.loading {
				m.loading = true
				cmds = append(cmds, tapeCmd(m.brandi, m.path, m.tapeGoal.Value(), tapePresets[m.tapePreset], false))
			}
		case "s":
			if m.tab == tabTape && !m.loading {
				m.loading = true
				cmds = append(cmds, tapeCmd(m.brandi, m.path, m.tapeGoal.Value(), tapePresets[m.tapePreset], true))
			}
		case "m":
			if m.tab == tabGrowth && !m.loading {
				m.loading = true
				cmds = append(cmds, growthCmd(m.brandi, m.path))
			}
		case "a", "x":
			if m.tab == tabGrowth && !m.loading {
				for _, draft := range m.growthQueue {
					if draft.Status == "pending" {
						draftCopy := draft
						m.growthConfirm = &draftCopy
						m.growthApprove = msg.String() == "a"
						m.growthConfirmStep = 0
						m.growthConfirmation.SetValue("")
						m.growthDestination.SetValue("")
						m.growthConfirmation.Focus()
						cmds = append(cmds, textinput.Blink)
						break
					}
				}
			}
		case "r":
			if !m.loading {
				m.loading = true
				cmds = append(cmds, runLintCmd(m.brandi, m.path))
			}
		case "f":
			if m.tab == tabOverview {
				m.projectsOK = false
				cmds = append(cmds, projectCmd(m.brandi, m.workspace))
			}
		case "W":
			if m.tab == tabSocial {
				m.wifiWizard = true
				m.wifiStep = 0
				m.wifiStatus = ""
				m.focusWifiInput()
				cmds = append(cmds, textinput.Blink)
			}
		case "c":
			if m.tab == tabSocial {
				m.accountWizard = true
				m.accountStep = 0
				m.accountStatus = ""
				m.focusAccountInput()
				cmds = append(cmds, textinput.Blink)
			}
		case "[", "]":
			if m.tab == tabSocial && len(m.socialAccounts) > 0 {
				delta := 1
				if msg.String() == "[" {
					delta = len(m.socialAccounts) - 1
				}
				m.socialAccountIndex = (m.socialAccountIndex + delta) % len(m.socialAccounts)
				m.disconnectArmed = ""
			}
		case "D":
			if m.tab == tabSocial && len(m.socialAccounts) > 0 && !m.loading {
				id := m.socialAccounts[m.socialAccountIndex].ID
				if m.disconnectArmed == id {
					m.loading = true
					m.disconnectArmed = ""
					cmds = append(cmds, accountDisconnectCmd(m.brandi, m.path, id))
				} else {
					m.disconnectArmed = id
					m.accountStatus = "press D again to disconnect " + id
				}
			}
		case "d":
			cmds = append(cmds, toggleDaemonCmd(m.brandi, m.path, m.daemon.Running))
		case "w":
			m.watch = !m.watch
		case "j", "down", "k", "up", "pgdown", "pgup", "g", "G":
			m.scrollActive(msg)
		}

	case animTickMsg:
		m.ticks++
		if m.ticks%5 == 0 {
			m.blink = !m.blink
		}
		// Ease the displayed score toward the latest target.
		diff := m.target - m.displayed
		if diff > 0.5 || diff < -0.5 {
			m.displayed += diff * 0.25
		} else {
			m.displayed = m.target
		}
		cmds = append(cmds, animTick())

	case watchTickMsg:
		if m.watch && !m.loading {
			m.loading = true
			cmds = append(cmds, runLintCmd(m.brandi, m.path))
		}
		cmds = append(cmds, watchTick())

	case progress.FrameMsg:
		g, cmd := m.gauge.Update(msg)
		if gm, ok := g.(progress.Model); ok {
			m.gauge = gm
		}
		cmds = append(cmds, cmd)

	case lintResultMsg:
		m.loading = false
		m.lastLint = time.Now()
		if msg.err != nil {
			m.err = msg.err
		} else {
			m.err = nil
			m.report = msg.report
			m.target = float64(msg.report.Scores.Overall)
			cmds = append(cmds, m.gauge.SetPercent(m.target/100))
			m.refreshViewport()
		}
		m.history = msg.history
		m.daemon = msg.daemon

	case daemonResultMsg:
		if msg.err != nil {
			m.err = msg.err
		} else {
			// The daemon writes its own first report; re-lint shortly to
			// pick up fresh state either way.
			m.loading = true
			cmds = append(cmds, runLintCmd(m.brandi, m.path))
		}

	case socialResultMsg:
		if msg.err == nil {
			m.social = msg.text
			m.socialAccounts = msg.accounts
			if m.socialAccountIndex >= len(m.socialAccounts) {
				m.socialAccountIndex = 0
			}
			m.socialOK = true
			m.vpSocial.SetContent(msg.text)
		}
	case accountResultMsg:
		m.loading = false
		m.accountToken.SetValue("")
		if msg.err != nil {
			m.err = msg.err
			m.accountStatus = msg.err.Error()
			m.accountWizard = true
			m.focusAccountInput()
		} else {
			m.err = nil
			m.accountWizard = false
			m.socialAccounts = msg.accounts
			m.accountStatus = msg.status
			m.socialAccountIndex = 0
		}
	case tapeResultMsg:
		m.loading = false
		if msg.err != nil {
			m.err = msg.err
		} else {
			m.err = nil
			m.tapeDraft = msg.draft
			m.tapeSaved = msg.saved
			m.vpTape.SetContent(renderTapeDraft(msg.draft))
		}
	case growthResultMsg:
		m.loading = false
		if msg.err != nil {
			m.err = msg.err
		} else {
			m.err = nil
			m.growth = msg.text
			m.growthQueue = msg.queue
			m.growthOK = true
			m.vpGrowth.SetContent(msg.text)
		}
	case growthDecisionMsg:
		if msg.err != nil {
			m.loading = false
			m.err = msg.err
		} else {
			cmds = append(cmds, growthCmd(m.brandi, m.path))
		}
	case projectResultMsg:
		if msg.err != nil {
			m.err = msg.err
		} else {
			m.projects = msg.projects
			m.projectsOK = true
		}
	case wifiResultMsg:
		m.loading = false
		if msg.err != nil {
			m.err = msg.err
			m.wifiStatus = msg.err.Error()
			m.wifiWizard = true
			m.focusWifiInput()
		} else {
			m.err = nil
			m.wifiStatus = msg.text
			m.wifiWizard = false
			m.wifiCode.SetValue("")
			cmds = append(cmds, socialCmd(m.brandi, m.path))
		}
	}

	return m, tea.Batch(cmds...)
}

func (m *model) blurWifiInputs() {
	m.wifiPair.Blur()
	m.wifiCode.Blur()
	m.wifiConnect.Blur()
}

func (m *model) focusWifiInput() {
	m.blurWifiInputs()
	switch m.wifiStep {
	case 0:
		m.wifiPair.Focus()
	case 1:
		m.wifiCode.Focus()
	default:
		m.wifiConnect.Focus()
	}
}

func (m *model) blurAccountInputs() {
	m.accountHandle.Blur()
	m.accountDisplayName.Blur()
	m.accountProfileURL.Blur()
	m.accountCredential.Blur()
	m.accountToken.Blur()
	m.accountBio.Blur()
}

func (m *model) focusAccountInput() {
	m.blurAccountInputs()
	switch m.accountStep {
	case 1:
		m.accountHandle.Focus()
	case 2:
		m.accountDisplayName.Focus()
	case 3:
		m.accountProfileURL.Focus()
	case 4:
		m.accountCredential.Focus()
	case 5:
		m.accountToken.Focus()
	case 6:
		m.accountBio.Focus()
	}
}

// scrollActive forwards scroll keys to the viewport of the active tab.
func (m *model) scrollActive(msg tea.KeyMsg) {
	switch m.tab {
	case tabOverview:
		m.vpOverview, _ = m.vpOverview.Update(msg)
	case tabFindings:
		m.vp, _ = m.vp.Update(msg)
	case tabSocial:
		m.vpSocial, _ = m.vpSocial.Update(msg)
	case tabTape:
		m.vpTape, _ = m.vpTape.Update(msg)
	case tabGrowth:
		m.vpGrowth, _ = m.vpGrowth.Update(msg)
	}
}

// refreshViewport rebuilds the findings viewport content after a new report.
func (m *model) refreshViewport() {
	if m.report == nil {
		return
	}
	y := m.vp.YOffset
	m.vp.SetContent(renderFindings(m.report))
	m.vp.YOffset = y
}

func (m model) View() string {
	if m.width > 0 && m.width < 50 {
		return "\n  Brandi — terminal too narrow (need 50+ columns)\n"
	}
	header := m.headerView()
	tabs := m.tabsView()
	footer := m.footerView()

	var content string
	switch m.tab {
	case tabOverview:
		content = m.overviewView()
	case tabFindings:
		content = m.findingsView()
	case tabRules:
		content = m.rulesView()
	case tabSocial:
		content = m.socialView()
	case tabTape:
		content = m.tapeView()
	case tabGrowth:
		content = m.growthView()
	}
	return lipglossJoinVertical(header, tabs, content, footer)
}

func lipglossJoinVertical(parts ...string) string {
	out := ""
	for i, p := range parts {
		if i > 0 {
			out += "\n"
		}
		out += p
	}
	return out
}

var _ = fmt.Sprintf // keep fmt imported for helpers in other files
