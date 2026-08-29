package main

// Data layer: drives the brandi binary and parses its machine-readable
// output. Everything the TUI shows comes from `brandi lint --format json`,
// `brandi social graph/plan`, or the daemon state files under
// `.brandi/state/` — the TUI never re-implements lint logic.

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

// Scores mirrors the `scores` object of the JSON lint report.
type Scores struct {
	Overall  uint32            `json:"overall"`
	ByKind   map[string]uint32 `json:"by_kind"`
	Errors   int               `json:"errors"`
	Warnings int               `json:"warnings"`
	Infos    int               `json:"infos"`
}

// Finding mirrors one JSON finding. Kind and Line may be null.
type Finding struct {
	RuleID     string  `json:"rule_id"`
	Severity   string  `json:"severity"` // Error | Warning | Info
	Kind       *string `json:"kind"`
	Path       string  `json:"path"`
	Line       *int    `json:"line"`
	Message    string  `json:"message"`
	Suggestion *string `json:"suggestion"`
}

// Report mirrors `brandi lint --format json`.
type Report struct {
	Root          string         `json:"root"`
	Timestamp     string         `json:"timestamp"`
	Scores        Scores         `json:"scores"`
	SurfaceCounts map[string]int `json:"surface_counts"`
	Findings      []Finding      `json:"findings"`
}

// HistoryEntry mirrors one line of `.brandi/state/history.jsonl`.
type HistoryEntry struct {
	Ts           string `json:"ts"`
	ScoringModel string `json:"scoring_model"`
	Score        int    `json:"score"`
	Errors       int    `json:"errors"`
	Warnings     int    `json:"warnings"`
	Infos        int    `json:"infos"`
}

// DaemonState is the observed daemon status from the pidfile + /proc.
type DaemonState struct {
	Running bool
	PID     int
}

type ProjectSummary struct {
	Name       string
	Path       string
	Score      int
	Rating     string
	Status     string
	Detail     string
	Configured bool
}

type SocialAccountSource struct {
	Content  bool    `json:"content"`
	Metrics  bool    `json:"metrics"`
	LastSync *string `json:"last_sync"`
}

type SocialAccountSurface struct {
	Profile    bool `json:"profile"`
	Publishing bool `json:"publishing"`
}

type SocialAccount struct {
	ID                  string               `json:"id"`
	Provider            string               `json:"provider"`
	Handle              string               `json:"handle"`
	DisplayName         string               `json:"display_name"`
	ProfileURL          string               `json:"profile_url"`
	Bio                 string               `json:"bio"`
	CredentialEnv       string               `json:"credential_env"`
	ConnectedAt         string               `json:"connected_at"`
	Source              SocialAccountSource  `json:"source"`
	Surface             SocialAccountSurface `json:"surface"`
	CredentialAvailable bool                 `json:"credential_available"`
}

type SocialAccountRegistry struct {
	SchemaVersion string          `json:"schema_version"`
	Accounts      []SocialAccount `json:"accounts"`
}

type SocialAccountInput struct {
	Provider      string
	Handle        string
	DisplayName   string
	ProfileURL    string
	Bio           string
	CredentialEnv string
	Token         string
}

// StarRating combines a five-star visual with the numeric evidence score.
func StarRating(score int, configured bool) string {
	if !configured {
		return "☆☆☆☆☆"
	}
	filled := (score + 10) / 20
	if filled < 0 {
		filled = 0
	}
	if filled > 5 {
		filled = 5
	}
	return strings.Repeat("⭐", filled) + strings.Repeat("☆", 5-filled)
}

func DiscoverProjects(brandi, workspace string) ([]ProjectSummary, error) {
	workspace, err := filepath.Abs(workspace)
	if err != nil {
		return nil, err
	}
	ignored := map[string]bool{
		".git": true, ".cache": true, ".cargo": true, ".rustup": true,
		"node_modules": true, "target": true, "vendor": true, "dist": true,
		"build": true, ".venv": true, "__pycache__": true,
	}
	projects := map[string]bool{}
	err = filepath.WalkDir(workspace, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return nil
		}
		if !entry.IsDir() {
			return nil
		}
		if path != workspace && (ignored[entry.Name()] || strings.HasPrefix(entry.Name(), ".")) {
			return filepath.SkipDir
		}
		relative, relErr := filepath.Rel(workspace, path)
		if relErr == nil && relative != "." && strings.Count(relative, string(os.PathSeparator)) >= 5 {
			return filepath.SkipDir
		}
		if isProjectDirectory(path) {
			projects[path] = true
		}
		return nil
	})
	if err != nil {
		return nil, err
	}

	paths := make([]string, 0, len(projects))
	for path := range projects {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	result := make([]ProjectSummary, 0, len(paths))
	for _, path := range paths {
		summary := ProjectSummary{Name: filepath.Base(path), Path: path}
		summary.Configured = hasGenome(path)
		if !summary.Configured {
			summary.Rating = StarRating(0, false)
			summary.Status = "⚪ unconfigured"
			summary.Detail = "add .brandi/ to measure coherence"
			result = append(result, summary)
			continue
		}
		if history := LoadHistory(path); len(history) > 0 {
			latest := history[len(history)-1]
			summary.Score = latest.Score
			summary.Detail = fmt.Sprintf("%d errors · %d warnings · cached daemon evidence", latest.Errors, latest.Warnings)
		} else if report, lintErr := RunLint(brandi, path); lintErr == nil {
			summary.Score = int(report.Scores.Overall)
			summary.Detail = fmt.Sprintf("%d errors · %d warnings · live evidence", report.Scores.Errors, report.Scores.Warnings)
		} else {
			summary.Rating = "☆☆☆☆☆"
			summary.Status = "⚫ unavailable"
			summary.Detail = lintErr.Error()
			result = append(result, summary)
			continue
		}
		summary.Rating = StarRating(summary.Score, true)
		switch {
		case summary.Score >= 80:
			summary.Status = "✅ aligned"
		case summary.Score >= 60:
			summary.Status = "🟡 drifting"
		default:
			summary.Status = "🔴 needs attention"
		}
		result = append(result, summary)
	}
	return result, nil
}

func isProjectDirectory(path string) bool {
	for _, marker := range []string{".git", "Cargo.toml", "go.mod", "package.json", "pyproject.toml", "setup.py"} {
		if _, err := os.Stat(filepath.Join(path, marker)); err == nil {
			return true
		}
	}
	return hasGenome(path)
}

type TapeIssue struct {
	Level   string `json:"level"`
	Code    string `json:"code"`
	Message string `json:"message"`
	Line    *int   `json:"line"`
}

type TapePlan struct {
	Title  string `json:"title"`
	Goal   string `json:"goal"`
	Scenes []struct {
		Title   string `json:"title"`
		Command struct {
			Program string   `json:"program"`
			Args    []string `json:"args"`
		} `json:"command"`
		PauseMS   uint64 `json:"pause_ms"`
		Rationale string `json:"rationale"`
	} `json:"scenes"`
}

type TapeDraft struct {
	Provider string      `json:"provider"`
	Plan     TapePlan    `json:"plan"`
	Tape     string      `json:"tape"`
	Critique []TapeIssue `json:"critique"`
}

type ContentDraft struct {
	ID           string `json:"id"`
	Status       string `json:"status"`
	Channel      string `json:"channel"`
	Content      string `json:"content"`
	RevisionHash string `json:"revision_hash"`
}

func GenerateTape(brandi, path, goal, preset string, save bool) (*TapeDraft, error) {
	args := []string{"tape", "generate", "--path", path, "--goal", goal, "--preset", preset, "--slug", "brandi-demo", "--format", "json"}
	if save {
		args = append(args, "--output", "demos/brandi-demo.tape", "--force")
	}
	out, err := exec.Command(brandi, args...).CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("tape generation: %s", strings.TrimSpace(string(out)))
	}
	var draft TapeDraft
	if err := json.Unmarshal(out, &draft); err != nil {
		return nil, fmt.Errorf("parsing tape draft: %w", err)
	}
	return &draft, nil
}

func GrowthSummary(brandi, path string) (string, []ContentDraft, error) {
	sections := []struct {
		name string
		args []string
	}{
		{"KAPTAIND MILESTONES", []string{"promotion", "milestones", "--path", path, "--format", "json"}},
		{"PROMOTION PLAN", []string{"promotion", "plan", "--path", path, "--format", "json"}},
		{"METRICS", []string{"promotion", "stats", "--path", path, "--format", "json"}},
		{"APPROVAL QUEUE", []string{"promotion", "queue", "--path", path, "--format", "json"}},
	}
	var b strings.Builder
	var queue []ContentDraft
	for _, section := range sections {
		out, err := exec.Command(brandi, section.args...).CombinedOutput()
		if err != nil {
			return "", nil, fmt.Errorf("%s: %s", section.name, strings.TrimSpace(string(out)))
		}
		if section.name == "APPROVAL QUEUE" {
			if err := json.Unmarshal(out, &queue); err != nil {
				return "", nil, fmt.Errorf("parsing approval queue: %w", err)
			}
		}
		b.WriteString(section.name + "\n" + string(out) + "\n")
	}
	return b.String(), queue, nil
}

func DecideGrowthDraft(brandi, path, id, confirmation, destination string, approve bool) error {
	args := growthDecisionArgs(path, id, confirmation, destination, approve)
	out, err := exec.Command(brandi, args...).CombinedOutput()
	if err != nil {
		return fmt.Errorf("%s draft: %s", args[1], strings.TrimSpace(string(out)))
	}
	return nil
}

func growthDecisionArgs(path, id, confirmation, destination string, approve bool) []string {
	command := "reject"
	if approve {
		command = "approve"
	}
	args := []string{"promotion", command, id, "--path", path, "--confirm", confirmation}
	if approve {
		args = append(args, "--destination", destination)
	}
	return args
}

// ruleCatalog mirrors rules::rule_catalog() in the Rust crate. The JSON
// report only contains rules that produced findings, so the full list is
// kept here to render passing rules. Keep in sync with src/rules.rs.
var ruleCatalog = []struct{ ID, Desc string }{
	{"prohibited-term", "prohibited words and phrases"},
	{"terminology-variant", "banned variants, former names, casing drift"},
	{"error-prefix", "machine-style error prefixes and numeric codes"},
	{"error-actionable", "error messages without a next step"},
	{"exclamation-limit", "exclamation marks beyond the voice budget"},
	{"sentence-case-headings", "Title Case instead of sentence case"},
	{"readme-mission", "README introduction missing product or mission"},
	{"readme-image-refs", "markdown image references that do not resolve"},
	{"palette-adherence", "colors outside the brand palette tolerance"},
	{"readme-h1", "README not opening with an H1 naming the product"},
}

// parseReport decodes a JSON lint report.
func parseReport(data []byte) (*Report, error) {
	var r Report
	if err := json.Unmarshal(data, &r); err != nil {
		return nil, fmt.Errorf("parsing report: %w", err)
	}
	return &r, nil
}

// RunLint executes `brandi lint --format json` and parses the report.
// Exit code 1 (lint gate failed) is tolerated: the report is still on
// stdout and remains useful.
func RunLint(brandi, path string) (*Report, error) {
	cmd := exec.Command(brandi, "lint", "--format", "json", "--path", path)
	out, err := cmd.Output()
	if err != nil {
		var exitErr *exec.ExitError
		if !errors.As(err, &exitErr) || len(out) == 0 {
			stderr := ""
			if exitErr != nil {
				stderr = strings.TrimSpace(string(exitErr.Stderr))
			}
			return nil, fmt.Errorf("brandi lint: %v %s", err, stderr)
		}
	}
	return parseReport(out)
}

// LoadHistory reads the daemon's score history; nil when none exists.
func LoadHistory(path string) []HistoryEntry {
	data, err := os.ReadFile(filepath.Join(path, ".brandi", "state", "history.jsonl"))
	if err != nil {
		return nil
	}
	var out []HistoryEntry
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		if line == "" {
			continue
		}
		var e HistoryEntry
		if json.Unmarshal([]byte(line), &e) == nil {
			out = append(out, e)
		}
	}
	return out
}

// DaemonStatus reports whether a brandi daemon is alive for path.
func DaemonStatus(path string) DaemonState {
	data, err := os.ReadFile(filepath.Join(path, ".brandi", "state", "daemon.pid"))
	if err != nil {
		return DaemonState{}
	}
	pid, err := strconv.Atoi(strings.TrimSpace(string(data)))
	if err != nil || pid <= 0 {
		return DaemonState{}
	}
	if _, err := os.Stat(fmt.Sprintf("/proc/%d", pid)); err != nil {
		return DaemonState{}
	}
	return DaemonState{Running: true, PID: pid}
}

// ToggleDaemon starts or stops the daemon via the brandi CLI.
func ToggleDaemon(brandi, path string, running bool) error {
	action := "start"
	if running {
		action = "stop"
	}
	cmd := exec.Command(brandi, "daemon", action, "--path", path)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("daemon %s: %s", action, strings.TrimSpace(string(out)))
	}
	return nil
}

// SocialGraph returns strategy plus read-only ADB device/target readiness.
func SocialGraph(brandi, path string) (string, error) {
	graph, err := exec.Command(brandi, "social", "graph", "--path", path).Output()
	if err != nil {
		return "", fmt.Errorf("social graph: %w", err)
	}
	plan, _ := exec.Command(brandi, "social", "plan", "--path", path).Output()
	adb, adbErr := exec.Command(brandi, "social", "adb", "status", "--path", path, "--format", "json").CombinedOutput()
	if adbErr != nil {
		adb = []byte("ADB SOCIAL\nUnavailable: " + strings.TrimSpace(string(adb)))
	} else {
		adb = append([]byte("ADB SOCIAL · read-only readiness\n"), adb...)
	}
	return string(graph) + "\n" + string(plan) + "\n" + string(adb), nil
}

func LoadSocialAccounts(brandi, path string) ([]SocialAccount, error) {
	out, err := exec.Command(brandi, "--format", "json", "social", "accounts", "list", "--path", path).CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("social accounts: %s", strings.TrimSpace(string(out)))
	}
	var registry SocialAccountRegistry
	if err := json.Unmarshal(out, &registry); err != nil {
		return nil, fmt.Errorf("social accounts JSON: %w", err)
	}
	return registry.Accounts, nil
}

func ConnectSocialAccount(brandi, path string, input SocialAccountInput) ([]SocialAccount, error) {
	args := socialAccountConnectArgs(path, input)
	cmd := exec.Command(brandi, args...)
	cmd.Env = os.Environ()
	if input.Token != "" {
		cmd.Env = append(cmd.Env, input.CredentialEnv+"="+input.Token)
	}
	out, err := cmd.CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("account sign-in: %s", strings.TrimSpace(string(out)))
	}
	if input.Token != "" {
		_ = os.Setenv(input.CredentialEnv, input.Token)
	}
	var registry SocialAccountRegistry
	if err := json.Unmarshal(out, &registry); err != nil {
		return nil, fmt.Errorf("account sign-in JSON: %w", err)
	}
	return registry.Accounts, nil
}

func socialAccountConnectArgs(path string, input SocialAccountInput) []string {
	return []string{
		"--format", "json", "social", "accounts", "connect",
		"--provider", input.Provider,
		"--handle", input.Handle,
		"--display-name", input.DisplayName,
		"--profile-url", input.ProfileURL,
		"--bio", input.Bio,
		"--credential-env", input.CredentialEnv,
		"--path", path,
	}
}

func DisconnectSocialAccount(brandi, path, id string) ([]SocialAccount, error) {
	out, err := exec.Command(
		brandi, "--format", "json", "social", "accounts", "disconnect", id,
		"--confirm", id, "--path", path,
	).CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("account disconnect: %s", strings.TrimSpace(string(out)))
	}
	var registry SocialAccountRegistry
	if err := json.Unmarshal(out, &registry); err != nil {
		return nil, fmt.Errorf("account disconnect JSON: %w", err)
	}
	return registry.Accounts, nil
}

// sparkline renders scores as a bar sparkline (▁▂▃▄▅▆▇█).
func sparkline(scores []int) string {
	if len(scores) == 0 {
		return ""
	}
	blocks := []rune("▁▂▃▄▅▆▇█")
	var b strings.Builder
	for _, s := range scores {
		idx := s * (len(blocks) - 1) / 100
		if idx < 0 {
			idx = 0
		}
		if idx > len(blocks)-1 {
			idx = len(blocks) - 1
		}
		b.WriteRune(blocks[idx])
	}
	return b.String()
}
