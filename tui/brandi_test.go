package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/charmbracelet/bubbles/textinput"
)

const sampleReport = `{
  "root": "/proj",
  "timestamp": "2026-07-17T18:38:30+01:00",
  "scores": {
    "overall": 42,
    "by_kind": {"repo_doc": 56, "style": 96, "ui_string": 90},
    "errors": 2,
    "warnings": 9,
    "infos": 2
  },
  "surface_counts": {"repo_doc": 7, "style": 2, "ui_string": 3},
  "findings": [
    {
      "rule_id": "prohibited-term",
      "severity": "Error",
      "kind": "repo_doc",
      "path": "/proj/README.md",
      "line": 1,
      "message": "'amazing' is prohibited (empty_hype)",
      "suggestion": "remove or rephrase 'amazing'"
    },
    {
      "rule_id": "readme-mission",
      "severity": "Warning",
      "kind": "repo_doc",
      "path": "/proj/README.md",
      "line": null,
      "message": "README introduction is missing the product name",
      "suggestion": null
    }
  ]
}`

func TestParseReport(t *testing.T) {
	r, err := parseReport([]byte(sampleReport))
	if err != nil {
		t.Fatalf("parseReport: %v", err)
	}
	if r.Scores.Overall != 42 {
		t.Errorf("overall = %d, want 42", r.Scores.Overall)
	}
	if r.Scores.ByKind["ui_string"] != 90 {
		t.Errorf("by_kind ui_string = %d, want 90", r.Scores.ByKind["ui_string"])
	}
	if len(r.Findings) != 2 {
		t.Fatalf("findings = %d, want 2", len(r.Findings))
	}
	f := r.Findings[0]
	if f.RuleID != "prohibited-term" || f.Severity != "Error" {
		t.Errorf("unexpected finding: %+v", f)
	}
	// brandi's JSON output uses snake_case for `kind`, matching the
	// `by_kind`/`surface_counts` keys above — it used to serialize as
	// default PascalCase ("RepoDoc"), inconsistent with those maps.
	if f.Kind == nil || *f.Kind != "repo_doc" {
		t.Errorf("kind = %v, want repo_doc", f.Kind)
	}
	if f.Line == nil || *f.Line != 1 {
		t.Errorf("line = %v, want 1", f.Line)
	}
	// Null line / suggestion must decode without error.
	g := r.Findings[1]
	if g.Line != nil || g.Suggestion != nil {
		t.Errorf("expected null line/suggestion, got %+v", g)
	}
}

func TestSparkline(t *testing.T) {
	if got := sparkline(nil); got != "" {
		t.Errorf("empty scores should give empty sparkline, got %q", got)
	}
	got := sparkline([]int{0, 50, 100})
	runes := []rune(got)
	if len(runes) != 3 {
		t.Fatalf("sparkline len = %d, want 3", len(runes))
	}
	if runes[0] != '▁' || runes[2] != '█' {
		t.Errorf("sparkline extremes = %q, want ▁…█", got)
	}
	if runes[1] != '▄' {
		t.Errorf("mid score 50 = %c, want ▄", runes[1])
	}
}

func TestRenderFindingsGroupsByPath(t *testing.T) {
	r, err := parseReport([]byte(sampleReport))
	if err != nil {
		t.Fatalf("parseReport: %v", err)
	}
	out := renderFindings(r)
	if !strings.Contains(out, "/proj/README.md") {
		t.Error("findings output should contain the path header")
	}
	if !strings.Contains(out, "L1") || !strings.Contains(out, "prohibited-term") {
		t.Error("findings output should contain line number and rule id")
	}
	if !strings.Contains(out, "→ remove or rephrase 'amazing'") {
		t.Error("findings output should contain the suggestion")
	}
}

func TestRenderFindingsEmpty(t *testing.T) {
	r := &Report{Findings: nil}
	if out := renderFindings(r); !strings.Contains(out, "no findings") {
		t.Errorf("empty findings should render a pass message, got %q", out)
	}
}

func TestRuleCatalogMatchesTen(t *testing.T) {
	if len(ruleCatalog) != 10 {
		t.Errorf("ruleCatalog has %d entries, want 10 (sync with src/rules.rs)", len(ruleCatalog))
	}
}

func TestTapeDraftContract(t *testing.T) {
	data := []byte(`{"provider":"deterministic-fallback","plan":{"title":"Demo","goal":"show it","scenes":[{"title":"Scan","command":{"program":"brandi","args":["scan"]},"pause_ms":1800,"rationale":"evidence"}]},"tape":"Output demos/demo.gif\n","critique":[]}`)
	var draft TapeDraft
	if err := json.Unmarshal(data, &draft); err != nil {
		t.Fatalf("tape draft: %v", err)
	}
	command := draft.Plan.Scenes[0].Command
	if draft.Provider != "deterministic-fallback" || command.Program != "brandi" || len(command.Args) != 1 || command.Args[0] != "scan" {
		t.Fatalf("unexpected tape contract: %+v", draft)
	}
}

func TestGrowthDecisionArgsRequireExactConfirmationAndDestination(t *testing.T) {
	approve := growthDecisionArgs("/project", "draft-1", "sha256:abc", "telegram:42", true)
	wantApprove := []string{"promotion", "approve", "draft-1", "--path", "/project", "--confirm", "sha256:abc", "--destination", "telegram:42"}
	if strings.Join(approve, "|") != strings.Join(wantApprove, "|") {
		t.Fatalf("approve args = %v, want %v", approve, wantApprove)
	}
	reject := growthDecisionArgs("/project", "draft-1", "draft-1", "", false)
	if strings.Contains(strings.Join(reject, " "), "--destination") {
		t.Fatalf("reject unexpectedly carries a destination: %v", reject)
	}
}

func TestControlRoomTabs(t *testing.T) {
	want := []string{"Overview", "Findings", "Rules", "Social", "Tape Studio", "Growth"}
	if len(tabNames) != len(want) {
		t.Fatalf("tabs = %v", tabNames)
	}
	for index := range want {
		if tabNames[index] != want[index] {
			t.Fatalf("tab %d = %q, want %q", index, tabNames[index], want[index])
		}
	}
}

func TestSocialLaunchSelectsSocialTab(t *testing.T) {
	if got := parseStartTab("social"); got != tabSocial {
		t.Fatalf("start tab = %v, want social", got)
	}
	if got := parseStartTab("unknown"); got != tabOverview {
		t.Fatalf("unknown start tab = %v, want overview", got)
	}
}

func TestDiscoverProjectRootFindsNamedWorkspaceChild(t *testing.T) {
	workspace := t.TempDir()
	genome := filepath.Join(workspace, "brandi", ".brandi")
	if err := os.MkdirAll(genome, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(genome, "identity.yaml"), []byte("product: {}\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if got := discoverProjectRoot(workspace); got != filepath.Join(workspace, "brandi") {
		t.Fatalf("project root = %q", got)
	}
}

func TestResolveBrandiPrefersProjectRelease(t *testing.T) {
	project := t.TempDir()
	release := filepath.Join(project, "target", "release")
	if err := os.MkdirAll(release, 0o755); err != nil {
		t.Fatal(err)
	}
	binary := filepath.Join(release, "brandi")
	if err := os.WriteFile(binary, []byte("binary"), 0o755); err != nil {
		t.Fatal(err)
	}
	if got := resolveBrandi("", project); got != binary {
		t.Fatalf("binary = %q, want %q", got, binary)
	}
}

func TestStarRatingAndUnconfiguredPortfolio(t *testing.T) {
	if got := StarRating(81, true); got != "⭐⭐⭐⭐☆" {
		t.Fatalf("rating = %q", got)
	}
	if got := StarRating(100, false); got != "☆☆☆☆☆" {
		t.Fatalf("unconfigured rating = %q", got)
	}
	workspace := t.TempDir()
	project := filepath.Join(workspace, "sample")
	if err := os.MkdirAll(project, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(project, "Cargo.toml"), []byte("[package]\nname='sample'\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	projects, err := DiscoverProjects("unused", workspace)
	if err != nil {
		t.Fatal(err)
	}
	if len(projects) != 1 || projects[0].Status != "⚪ unconfigured" {
		t.Fatalf("projects = %+v", projects)
	}
}

func TestTruncateAddsEllipsisOnlyWhenNeeded(t *testing.T) {
	if got := truncate("short", 20); got != "short" {
		t.Errorf("truncate should not touch a string under width, got %q", got)
	}
	if got := truncate("exactly-ten", 11); got != "exactly-ten" {
		t.Errorf("truncate should not touch a string exactly at width, got %q", got)
	}
	if got := truncate("this is a long project path", 10); got != "this is a…" {
		t.Errorf("truncate = %q, want ellipsis-terminated 10 runes", got)
	}
	if got := []rune(truncate("this is a long project path", 10)); len(got) != 10 {
		t.Errorf("truncated length = %d, want 10", len(got))
	}
}

func TestFooterAdvertisesDaemonAndWatchKeys(t *testing.T) {
	m := newModel("/project", "/workspace", "brandi", false)
	m.width = 120
	footer := m.footerView()
	for _, key := range []string{"d daemon", "w watch"} {
		if !strings.Contains(footer, key) {
			t.Errorf("footer missing %q: %q", key, footer)
		}
	}
}

func TestWifiWizardMasksCodeAndExplainsDistinctEndpoints(t *testing.T) {
	m := newModel("/project", "/workspace", "brandi", false)
	if m.wifiCode.EchoMode != textinput.EchoPassword {
		t.Fatalf("pairing code echo mode = %v, want password", m.wifiCode.EchoMode)
	}
	m.wifiWizard = true
	m.wifiStep = 1
	m.wifiCode.SetValue("123456")
	out := m.socialView()
	for _, want := range []string{"Wi-Fi connection wizard", "Wireless debugging", "Pair device with pairing code", "different ports", "never passed on the command line"} {
		if !strings.Contains(out, want) {
			t.Fatalf("wizard output missing %q: %q", want, out)
		}
	}
	if strings.Contains(out, "123456") {
		t.Fatal("wizard rendered the pairing code in clear text")
	}
}

func TestSocialSignInMasksTokenAndModelsBothRoles(t *testing.T) {
	m := newModel("/project", "/workspace", "brandi", false)
	if m.accountToken.EchoMode != textinput.EchoPassword {
		t.Fatalf("account token echo mode = %v, want password", m.accountToken.EchoMode)
	}
	m.accountWizard = true
	m.accountStep = 5
	m.accountToken.SetValue("social-secret")
	out := m.socialView()
	for _, want := range []string{"Social account sign-in", "source + governed surface", "never stored", "never passed in argv"} {
		if !strings.Contains(out, want) {
			t.Fatalf("account wizard missing %q: %q", want, out)
		}
	}
	if strings.Contains(out, "social-secret") {
		t.Fatal("account wizard rendered the session token in clear text")
	}
}

func TestSocialAccountConnectArgsNeverContainToken(t *testing.T) {
	input := SocialAccountInput{
		Provider: "mastodon", Handle: "brandi", DisplayName: "Brandi",
		ProfileURL: "https://social.example/@brandi", Bio: "Brand coherence",
		CredentialEnv: "MASTODON_ACCESS_TOKEN", Token: "social-secret",
	}
	args := socialAccountConnectArgs("/project", input)
	joined := strings.Join(args, " ")
	if strings.Contains(joined, input.Token) {
		t.Fatalf("token leaked into argv: %v", args)
	}
	if !strings.Contains(joined, input.CredentialEnv) {
		t.Fatalf("credential reference missing from argv: %v", args)
	}
}

func TestSocialAccountsRenderAsSourcesAndSurfaces(t *testing.T) {
	m := newModel("/project", "/workspace", "brandi", false)
	m.socialOK = true
	m.socialAccounts = []SocialAccount{{
		ID: "mastodon-brandi", Provider: "mastodon", Handle: "brandi",
		DisplayName: "Brandi", ProfileURL: "https://social.example/@brandi",
		CredentialEnv: "MASTODON_ACCESS_TOKEN", CredentialAvailable: true,
		Source:  SocialAccountSource{Content: true, Metrics: true},
		Surface: SocialAccountSurface{Profile: true, Publishing: true},
	}}
	out := m.socialView()
	for _, want := range []string{"Social accounts", "source", "surface", "content + metrics", "public profile + publishing"} {
		if !strings.Contains(out, want) {
			t.Fatalf("account view missing %q: %q", want, out)
		}
	}
}
