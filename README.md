# Brandi

*Brandi: brand coherence intelligence*

Brandi is a brand coherence intelligence layer: a daemon and CLI that treats branding as a systems property, not a marketing afterthought. It loads a machine-readable brand definition — a brief (who the product is and who it's for) and guidelines (how it sounds, looks, and what language it forbids), both kept in `.brandi/` — and lints every outward expression of a project against it: docs, README files, UI and CLI strings, colors, and image assets.

Brandi sits alongside engineering governance tools. Kaptaind asks "is this software evolving safely, traceably, according to release discipline?"; Brandi asks "does every outward expression of this software reinforce a coherent identity, purpose, and user experience?". Together with Fract they form a software organism immune system:

```
┌────────────────────────┬────────────────────────┬────────────────────────┐
│ Kaptaind               │ Fract                  │ Brandi                 │
├────────────────────────┼────────────────────────┼────────────────────────┤
│ protects software      │ optimises internal     │ protects external      │
│ evolution and          │ structure              │ perception and         │
│ versioning             │                        │ identity               │
├────────────────────────┼────────────────────────┼────────────────────────┤
│ prevents mutation      │ prevents structural    │ prevents identity      │
│ chaos                  │ decay                  │ drift                  │
└────────────────────────┴────────────────────────┴────────────────────────┘
```

Kaptaind prevents mutation chaos, Fract prevents structural decay, Brandi prevents identity drift. The mission, as the default brief states it: "Keep every outward expression of a product aligned with its identity."

## Why Brandi

Modern products leak identity through thousands of micro-decisions: button labels, error messages, CLI output, README tone, docs style, repo structure, landing pages, social posts, colors, icons, screenshots, release notes. Style guides try to hold all of that together and fail quietly, because nobody can check a document against a shipping product every day.

As AI-generated software becomes common, the bottleneck shifts from producing code to maintaining coherence. When generation is cheap, the hard part is making sure the product still sounds, looks, and behaves like itself across every surface. Brandi makes identity machine-checkable: the brief and guidelines are the constitution, the linter enforces it, and the daemon keeps enforcing it while the project moves.

## Features

- **Brand brief and guidelines.** A machine-readable brand definition in `.brandi/`: the brief covers identity and audience; the guidelines cover voice, visual language, and prohibited language. Partial files load with defaults; `brandi brief validate` and `brandi guidelines validate` check them.
- **Evidence-rich surface scanner.** Runs a transparent six-milestone pipeline, uses Bound to capture disposable and size-limited source evidence, and separates detection confidence from significance. Target-use identification reports whether evidence describes a CLI, TUI, GUI, web frontend, mobile app, or TV app; its Padagonia software ontology is fingerprinted and reused until relevant manifests, markers, or surface paths change. Every surface carries domain, subtype, provenance, structured audience exposure, relevance, authority, and provenance-adjusted semantic weight. Findings route back to their governing context; when Dreamseq is installed, `.dreams/*.dreams` files found from the current directory upward are included as read-only delivery targets. An optional `.brandiignore` at the project root (one path per line, `#` comments allowed) excludes specific files or directories from surface extraction.
- **Proposal engine.** Scans the same surfaces and spends a bounded generative budget on concrete before/after revisions for objects that are subject to stylisation.
- **Brand linter.** Ten ESLint-style rules with severities, suggestions, human or JSON output, and CI-friendly gates (`--fail-under`, `--strict`).
- **Asset intelligence.** Checks image assets against the guidelines: dimensions, dominant colors versus the palette, whitespace ratio. Typography, composition, and message clarity are reported as manual-review items — Brandi does not pretend an algorithm can judge them.
- **Repository branding.** README, CONTRIBUTING, issue templates, and release notes are first-class lint surfaces, checked like any other outward expression of the product.
- **Project-linked social accounts.** `brandi social` opens an account workspace where provider profiles are linked to one project and treated simultaneously as sources and surfaces: content/metrics evidence plus public brand identity. Tokens remain in environment variables or the current masked TUI session; project files store only credential references.
- **Social strategy engine.** Derives a capability → narrative → audience → format graph from the brief and renders content plans per audience segment.
- **Watch daemon.** Re-lints the project when surfaces change and keeps a score history, so identity drift shows up the day it is introduced.
- **Hot-pink terminal control room.** A Bubble Tea and Lip Gloss TUI adds Tape Studio for reasoned VHS demos and Growth for promotion, metrics, Kaptaind milestones, and approvals.
- **Safe VHS intelligence.** Structured model planning falls back deterministically, compiles to `.tape`, blocks mutating shell patterns, runs native `vhs validate`, confines saves to `demos/`, and renders only on an explicit command.
- **Evidence-led promotion.** Git, GitHub, and Telegram signals shape dynamic GitHub/Telegram plans; every snapshot and plan enters a durable Padagonia outbox with provenance.
- **Telegram hub or fleet.** One daemon-level bot can route across projects, or one supervisor can run an array of project bots. Q/A uses Groq Compound Mini over bounded read-only local evidence; publishing requires an administrator to approve the exact queued revision.
- **Kaptaind milestones.** Brandi consumes Kaptaind release indexes without changing them, scores novelty and release evidence, brand-lints milestone copy, and deduplicates approval drafts.

## Install and build

```bash
cargo build --release
cd tui && go build -o brandi-tui .
```

The binary is `target/release/brandi`. Put it on your `PATH` to run it as `brandi`.

## Quickstart

```bash
cd your-repo
brandi init            # scaffold .brandi/ with the brief and guidelines files
# edit .brandi/*.yaml to describe your product
brandi lint            # first coherence report
brandi propose         # suggested aesthetic amendments
brandi direct          # generate missing guidelines-required assets
brandi reshoot         # revise visual and non-code textual assets
brandi daemon start    # keep watching in the background
brandi daemon status
brandi daemon stop
./tui/brandi-tui --brandi ./target/release/brandi
```

## Command reference

Every command that acts on a project accepts `--path <DIR>` for the project root (default: the current directory). `brandi brief [PATH]` and `brandi guidelines [PATH]` instead accept an optional positional target and discover the nearest `.brandi/` directory at or above that target (or the current directory). The exceptions are commands with nothing project-scoped to point at: `brandi telegram run`/`status` (configured entirely by `--config`) and the `brandi social adb wifi` subcommands (pure device operations — pairing, connecting, disconnecting, bridging — that don't touch project state).

### `brandi init`

Scaffold a `.brandi/` brief and guidelines directory: the five YAML files plus an `assets/` folder. Brandi first derives the brief from bounded, root-level repository evidence (supported package metadata and the README title/intro); if it cannot establish both a product name and description, it uses the standard defaults. Initialization is idempotent — existing files are never overwritten.

```bash
brandi init
brandi init --path ./my-project
```

### `brandi brief [PATH]`

Display the associated project's merged brief (`identity.yaml` and `audience.yaml`). With no path, Brandi starts at the current directory and walks upward to the nearest `.brandi/`; a target path may be a project directory, nested directory, or file. Use `--format json` for a stable machine-readable document.

```bash
brandi brief
brandi brief ./src
```

### `brandi brief validate`

Validate the brief files (`identity.yaml`, `audience.yaml`) and report problems. Prints `brief OK` when clean, warnings for soft problems (empty product name, no audience segments), and fails on hard problems (missing files, invalid YAML).

```bash
brandi brief validate
```

### `brandi guidelines [PATH]`

Display the associated project's merged guidelines (`voice.yaml`, `visual.yaml`, `prohibited.yaml`, and optional `rules.yaml`) using the same upward project discovery as `brandi brief`.

```bash
brandi guidelines
brandi guidelines ./docs/guide.md
```

### `brandi guidelines validate`

Validate the guidelines files (`voice.yaml`, `visual.yaml`, `prohibited.yaml`, optional `rules.yaml`) and report problems. Prints `guidelines OK` when clean, warnings for soft problems (unset primary color, severity entry without a matching category), and fails on hard problems (missing files, invalid YAML, malformed palette colors, unknown severity values).

```bash
brandi guidelines validate
```

### `brandi scan`

Run the evidence-rich project inspection pipeline. With no `--path`, Brandi resolves the nearest project root at or above the current directory; an explicit `--path` must name the top-level project root itself. If that target has no `.brandi/`, an interactive terminal asks the user to choose init or skip, with skip as the default. Non-interactive runs never hang: select `--if-uninitialized init` to scaffold and continue or `--if-uninitialized skip` to emit a successful structured skip result. Brandi reports each milestone as it resolves scope, discovers surfaces, identifies target use through a reusable Padagonia ontology, captures bounded source snapshots with Bound, evaluates brief/guideline findings, and routes those findings to their governing `.brandi/` files. If `dreamseq` is installed, only `.dreams` files—whether project-local or in an ancestor directory—that reference the target project are listed as read-only delivery targets; unrelated dreams are ignored. Scan never rewrites briefs, guidelines, dreams, or project sources except when initialization is explicitly confirmed; it may refresh derived cache evidence under `.brandi/state/`.

Interactive human output uses 3form tables, semantic colour, and a spinner for each active milestone. The default view identifies the frontend form and its evidence, explains every targeted surface class with representative paths and audiences, then aggregates detected versus analysable surfaces by type, semantic/operational domain, provenance, and audience. Delivery rows name the governed surface kinds and why each brief, guideline, or Dreamseq file receives them. `--verbose` adds the line-by-line findings and surface evidence. Redirected and machine output stays static; JSON includes the full target summary, Padagonia reuse metadata, significance profiles, completed milestone ledger, Bound evidence totals, findings, and delivery indexes.

Generated agent instructions and repository machinery remain visible evidence, but their provenance lowers their semantic relevance and weight so they cannot dominate product identity analysis through volume alone. Test directories are no longer universally excluded: AST extraction retains each test assertion string and expected user-facing value while discarding unrelated test code and debug output. Exclusion diagnostics name the policy class, such as generated build artefacts, dependency corpus, Brandi configuration, or project `.brandiignore` policy.

```bash
brandi scan
brandi scan --path /srv/projects/widget
brandi scan --path /srv/projects/widget --if-uninitialized init
brandi scan --path /srv/projects/widget --if-uninitialized skip
brandi scan --format json
brandi scan --animation always --color always
```

### Global output controls

Every command and nested command accepts the same presentation controls before
or after the command path:

```bash
brandi --format json lint
brandi lint --format jsonl
brandi scan --color never --animation never --unicode never --width 80
brandi direct --fidelity fill --verbose
```

- `--format human|json|jsonl` selects styled human output, one JSON document,
  or compact newline-delimited JSON records. Foreground daemon and Telegram
  supervisors require JSONL rather than an unbounded JSON document.
- `--color auto|always|never`, `--animation auto|always|never`, and
  `--unicode auto|always|never` govern human output only. Machine output is
  always static and decoration-free.
- `--fidelity auto|static|storyboard|sketch|fill|print|show|movie` selects the
  visual fidelity. 3form provides the safe baseline; explicit Barbara-rich
  modes currently fall back to 3form until Barbara exposes its versioned live
  event protocol. `--verbose` reports that fallback on stderr.
- `--width 20..1000`, `--quiet`, and `--verbose` support deterministic output,
  silent successful operation (warnings/errors remain on stderr), and renderer
  diagnostics.

Automatic colour, Unicode, and animation require the corresponding output
stream to be a terminal. `NO_COLOR`, `TERM=dumb`, CI, redirection, and machine
formats select the static/plain path. Transient progress starts after 200 ms,
is capped at 10 redraws per second, and is cleared on success or failure.

### `brandi propose [--budget N]`

Scan the project, determine which discovered objects are subject to stylisation, and print aesthetically amended revisions. The command is a dry run: it does not edit files. Before proposal generation, every finding becomes a typed semantic object with a finding class, syntactic role, provenance, audience, confidence, relevance, authority, exposure, impact, and priority. Classification describes what the finding is; the multiplicative priority determines how much it matters. This keeps generated operational governance visible in scan and lint evidence while excluding low-relevance machinery from the proposal budget.

`--budget` caps the number of generated revisions (default: `12`), ranked by semantic priority rather than discovery order. Proposal operations are typed as replacements, rewrites, structural edits, or advisories. Heading rules retain heading syntax instead of inheriting sentence punctuation, while document-wide constraints report structured current/proposed counts. Colour revisions include exact 24-bit true-colour swatches beside both hex values when colour output is active, with an unambiguous hex fallback for static output; JSON also records the configured palette, nearest-colour strategy, role, and selection confidence. Typography is represented as a positive opportunity—not a violation—and supplies a specimen plus copy-ready inline font-family usage in CSS and HTML. Standard terminals cannot change font faces per span, so the inline usage is explicit rather than pretending the terminal glyphs use that family.

```bash
brandi propose
brandi propose --budget 5
```

Output looks like:

```
Brandi proposals (budget 5, generated 2)

1. ./README.md:3 [repo_doc] repository prose
   reason: 'awesome' is prohibited (empty_hype)
   operation: replacement · priority 82/100
   current: This awesome project helps teams.
   revised: This project helps teams.
2. ./style.css:12 [style] color token
   reason: off-palette color #ff0000
   operation: replacement · priority 76/100
   current: #ff0000
   revised: #1f2937
   colour: [#ff0000 swatch] #ff0000 → [#1f2937 swatch] #1f2937
   selection: configured · nearest_palette_colour · confidence 88%

Typography opportunity
   font: Inter · technical_modern
   specimen: Brandi makes every surface feel intentional — Aa Bb Cc 0123456789
   inline CSS: font-family: "Inter", ui-sans-serif, system-ui, sans-serif;
   inline HTML: <span style="font-family: &quot;Inter&quot;, ui-sans-serif, system-ui, sans-serif;">…</span>
```

### `brandi direct [--path DIR] [--portfolio]`

Generate assets that the guidelines require but the project does not yet have. Brandi calls Vasilis with the product identity, palette, typography, whitespace, and configured dimensions to create missing social cards and thumbnails. When an audience narrative requests `demo_video`, it also generates a validated `demos/<product>-demo.tape`; VHS rendering remains an explicit, separately confirmed operation.

```bash
brandi direct
brandi direct --path /srv/projects/widget
brandi direct --portfolio
brandi direct --portfolio --path /srv/projects
```

In portfolio mode, an omitted path means `HOME`. Existing asset classes are never overwritten by `direct`. Configure deterministic output locations with `visual.assets.social_card_path` and `thumbnail_path`; both must be project-relative `.svg` paths.

### `brandi reshoot [--path DIR] [--portfolio]`

Revise every discovered image, stylesheet colour token, and non-code documentation surface against the current brief and guidelines. Raster pixels and SVG colours are mapped onto the configured palette, while prose changes use the same evidence-backed revision engine as `propose`. Source-code strings, `.brandi/` configuration and state, generated files, dependencies, and hidden directories are excluded.

```bash
brandi reshoot --path ./my-project
brandi reshoot --portfolio
brandi reshoot --portfolio --path /srv/projects
```

Portfolio discovery is bounded to six levels and includes only projects whose required brief and guidelines files parse and validate. Processing continues after an individual project failure and returns exit status 1 with a per-project summary when any project fails.

### `brandi check <FILE>`

Lint a single file against the brief and guidelines and print a human report.

```bash
brandi check README.md
brandi check docs/getting-started.md
```

### `brandi lint [--format human|json|jsonl] [--fail-under N] [--strict]`

Lint the whole project against the brief and guidelines.

- `--format human|json|jsonl` — global output format (default: `human`)
- `--fail-under N` — exit 1 when the overall score is below `N` (default: `0`, no gate)
- `--strict` — exit 1 when any error-severity finding exists

```bash
brandi lint
brandi lint --fail-under 80 --strict
brandi lint --format json > lint-report.json
```

Human output looks like:

```
Brandi report — . (2026-07-17T11:43:35+01:00)

Surfaces scanned: repo_doc 1, ui_string 3

Findings:
./README.md
  ✗ L3 [prohibited-term] 'revolutionary' is prohibited (empty_hype)
    → remove or rephrase 'revolutionary'

✓ error-prefix — machine-style error prefixes and numeric error codes in UI strings
✓ palette-adherence — colors outside the brand palette tolerance

Errors: 1  Warnings: 0  Info: 0
Brand coherence: 90/100
  repo_doc 90  ui_string 100
```

### `brandi assets check <IMAGES...> [--kind auto|social-card|thumbnail|icon]`

Check image assets against the guidelines' asset spec. With `--kind auto` (the default), the spec is inferred from filename and content.

```bash
brandi assets check .brandi/assets/social-card.png --kind social-card
brandi assets check hero.png card.png --kind thumbnail
```

Output looks like:

```
Asset: social-card.png (kind: social-card)

Dimensions    1200x630 ✓ (expected 1200x630)
Brand colours 86% within palette
Whitespace    31% (min 25%) ✓
Dominant      #0e7c7b #1f2937 #f9fafb

Manual review: typography, composition, message clarity.
```

### `brandi assets list`

List brand image assets discovered in the project.

```bash
brandi assets list
```

### `brandi assets audit [--format human|json]`

Project-wide asset coherence and utilisation audit: palette adherence, file-size budget (`visual.assets.max_file_kb`), resolution fit for each asset's inferred class, exact and near-duplicate detection, icon-set consistency, and reference counting across project text files. Assets are classified filename-first, falling back to geometry for untyped names — the same classifier `assets check --kind auto` uses, so a file classifies the same way under either command. Score: 100 minus per-issue penalties, saturating at 0.

```bash
brandi assets audit
brandi assets audit --format json
```

### `brandi social [--path DIR]`

Open the full-screen Social account workspace. Press `c` to sign in to Mastodon, Bluesky, X, Instagram, LinkedIn, YouTube, or TikTok; `[`/`]` selects a linked account and pressing `D` twice removes its project link. The wizard accepts a masked provider-issued token for the current TUI session, or uses an already-exported credential environment variable. Secret values are placed only in the child process environment: they are never command arguments and never written to `.brandi/social-accounts.json`.

Each linked account has two explicit roles:

- **source** — content and metrics evidence that can inform planning and brand analysis;
- **surface** — the public display name, handle, bio, URL, and publishing identity governed by the project brief and guidelines.

The scanner injects those public profile fields as `social` surfaces even though ordinary `.brandi/` configuration remains excluded. Machine workflows can manage the same registry without opening a TUI:

```bash
export MASTODON_ACCESS_TOKEN='provider-issued-value'
brandi social accounts connect --provider mastodon --handle brandi \
  --display-name Brandi --profile-url https://social.example/@brandi \
  --credential-env MASTODON_ACCESS_TOKEN --path .
brandi --format json social accounts list --path .
brandi social accounts disconnect mastodon-brandi \
  --confirm mastodon-brandi --path .
```

### `brandi social graph`

Render the narrative graph from `audience.yaml`: capabilities mapped through narratives to audiences and formats.

```bash
brandi social graph
```

### `brandi social plan [--segment NAME]`

Render a content plan derived from the brief, optionally limited to one audience segment.

```bash
brandi social plan
brandi social plan --segment platform_engineers
```

### `brandi daemon run`

Run the watch daemon in the foreground. Useful under a process supervisor; stop with Ctrl-C.

```bash
brandi daemon run
```

### `brandi daemon start`

Start the watch daemon in the background.

```bash
brandi daemon start
```

### `brandi daemon stop`

Stop the background daemon.

```bash
brandi daemon stop
```

### `brandi daemon status`

Print the daemon status for the project.

```bash
brandi daemon status
```

### Tape Studio and `brandi tape`

The TUI's Tape Studio edits a goal, cycles deterministic presets, previews the structured plan and critique, and saves only after `s` is pressed. Generation never renders. The CLI exposes the same boundary:

```bash
brandi tape generate --goal "Show the release workflow" --preset product-tour
brandi tape generate --goal "Show the release workflow" --output demos/release.tape
brandi tape validate demos/release.tape
# First call previews the normalized plan and prints its SHA-256 token.
brandi tape render demos/release.tape
brandi tape render demos/release.tape --confirm render-<sha256-token>
```

Configure an OpenAI-compatible Responses endpoint in `.brandi/reasoning.yaml`. The API key is read from `api_key_env`; if configuration, credentials, or the provider are unavailable, generation uses the deterministic preset compiler. Tape scenes are typed executable/argument records. The policy accepts only `brandi scan`, `brandi lint`, bounded `brandi propose --budget`, and the two read-only social planning commands. Rendering fails closed without Bubblewrap, runs with no network or host credentials and a read-only host, writes first to a disposable directory, then promotes one bounded regular output file. Every attempt records the plan hash, OS-credential-bound approver, sandbox policy, VHS digest, timestamps, commands, and result in `.brandi/state/tape-executions.jsonl`.

### Growth, Padagonia, and monitoring

```bash
brandi promotion stats --format json
brandi promotion plan --format json
brandi promotion milestones
brandi promotion queue
brandi promotion sync
```

`.brandi/promotion.yaml` configures the GitHub repository, optional monorepo subpath, Padagonia endpoint, planning horizon, milestone threshold, and Telegram channel. Secrets stay in `GITHUB_TOKEN`, the configured Padagonia key variable, and the configured Telegram token variable. Brandi appends local JSONL records beneath `.brandi/state/` first; `promotion sync` replays the Padagonia outbox and retains failed records. It does not post promotion content automatically.

`promotion milestones` reads `.kaptaind/ship/index.json` and `.kaptaind/releases/index.json`. A qualifying event creates one immutable-revision draft. Repeated scans are idempotent.

### Telegram supervisor

Copy `examples/telegram.yaml` to `$XDG_CONFIG_HOME/brandi/telegram.yaml`, replace IDs, export the named token variables and `GROQ_API_KEY`, then validate or run it:

```bash
brandi telegram status --config "$XDG_CONFIG_HOME/brandi/telegram.yaml"
brandi telegram run --config "$XDG_CONFIG_HOME/brandi/telegram.yaml"
```

`mode: hub` uses the first token for one daemon-level bot and routes by configured chat or `/project <id> <command>`. `mode: fleet` starts one worker per project and token in the same process. Empty `allowed_users` is rejected unless `public_mode: true` is explicitly enabled; public mode exposes only the read-only viewer commands and prints a startup warning. Per-project `roles` separately assign planners, approvers, publishers, and administrators. Approval plus publication requires both roles and the command `/approve <id> <revision-hash>`; rejection uses `/reject <id> <revision-hash>`. Approvals expire after 15 minutes and bind to the exact `telegram:<chat-id>` destination.

Local approval and device actions derive identity from the OS UID and deny unless `.brandi/authorization.yaml` grants the minimum role. Copy `examples/authorization.yaml`, replace the UID with `id -u`, and assign only the required roles. Caller-supplied actor strings are not accepted. Allowed and denied decisions enter the hash-chained `.brandi/state/security-audit.jsonl` log.

### Workspace portfolio

Overview discovers project roots beneath the user's home directory and combines a five-star emoji rating with an evidence summary. Git repositories and common Rust, Go, Node, and Python manifests are listed; projects without `.brandi/` are marked unconfigured. Configured projects use the latest daemon evidence when available and otherwise receive a live lint.

```bash
brandi-tui --workspace /srv/projects --path /srv/projects/brandi
BRANDI_WORKSPACE_ROOT=/srv/projects brandi-tui
```

The walk skips build outputs, dependency directories, caches, and hidden directories, and is bounded to six directory levels. Press `f` on Overview to rescan.

### ADB social orchestration and publishing

Brandi can stage an approved content revision into an allowlisted Android app and, with a second exact confirmation, locate and tap that app's configured publish control. Copy `examples/social-adb.yaml` to `.brandi/social-adb.yaml`, then verify the package and selector against the installed app version.

```bash
brandi social adb status --path .
brandi social adb wifi connect 192.168.1.50:42131
BRANDI_ADB_PAIR_CODE=123456 brandi social adb wifi pair \
  --pair-endpoint 192.168.1.50:37123 \
  --connect-endpoint 192.168.1.50:42131
brandi social adb wifi disconnect 192.168.1.50:42131
# For older devices: authorize USB first, then explicitly enable TCP/IP ADB.
brandi social adb wifi bridge --device R58M1234 --port 5555
brandi promotion queue --path .
brandi promotion approve draft-kaptaind-ship-1-2-0-7 \
  --confirm sha256:<revision-hash> \
  --destination adb:x \
  --path .
brandi social adb stage draft-kaptaind-ship-1-2-0-7 --target x --path .
brandi social adb publish draft-kaptaind-ship-1-2-0-7 \
  --target x \
  --confirm draft-kaptaind-ship-1-2-0-7 \
  --path .
```

The Social tab in `brandi-tui` shows whether each attached device is using USB, Wi-Fi, or an emulator. Press `W` there to open the hot-pink Wi-Fi wizard. On Android 11 and newer, open **Settings → Developer options → Wireless debugging → Pair device with pairing code** and enter the pairing endpoint, six-digit code, and debug endpoint. Android often assigns different ports to the pairing and debug endpoints. The wizard masks the code and Brandi feeds it to `adb pair` over standard input instead of putting it in the process argument list.

`wifi bridge` supports the older USB-to-TCP/IP flow. The selected device must be online over USB and connected to the same Wi-Fi network; Brandi discovers its Wi-Fi address or accepts `--host`. Pairing, connecting, bridging, and disconnecting are explicit commands—status checks never change device connectivity.

Safety boundaries:

- Only unexpired queue revisions approved for the exact `adb:<target>` destination may be staged.
- Stage uses `android.intent.action.SEND`; it opens a composer but never taps publish.
- Publish requires `--confirm` to equal the immutable draft ID exactly; local publisher and device-operator roles are both required.
- App packages and UI selectors are explicit allowlists; selector misses tap nothing.
- Commands are passed to `adb` as argument arrays, not through a shell.
- Successful staging and publishing append evidence to `.brandi/state/adb-social.jsonl`; authorization and delivery decisions also enter the chained security audit.

## Brief reference

The brief lives in `.brandi/` as two YAML files: `identity.yaml` and `audience.yaml`. Every field has a default, so partial files load cleanly; a missing file is an error. `brandi init` writes a repository-derived brief when local metadata and README evidence establish its identity, otherwise it writes the defaults shown below; it never overwrites existing files.

### identity.yaml

Who the product is: names, archetypes, canonical terminology.

```yaml
product:
  name: Brandi
  tagline: "Brand coherence intelligence"
  mission: "Keep every outward expression of a product aligned with its identity."
  aliases: []
  former_names: []
archetype: [explorer, engineer, challenger]
terminology:
  canonical: {}
  banned_variants: {}
```

- `product.name` — canonical product name, written exactly as it should appear everywhere.
- `product.tagline` — one-line descriptor.
- `product.mission` — mission statement; the `readme-mission` rule checks that the README carries it.
- `product.aliases` — accepted alternative names.
- `product.former_names` — outdated names that should no longer appear.
- `archetype` — brand archetypes; guidance for voice and narrative.
- `terminology.canonical` — map of concepts to their canonical terms.
- `terminology.banned_variants` — variants that must not be used; the `terminology-variant` rule flags a banned variant and points at the canonical term.

### audience.yaml

Who the brand speaks to and the stories it tells.

```yaml
segments:
  - name: platform_engineers
    description: "Teams responsible for engineering governance"
    pains: ["identity drift", "inconsistent UX copy"]
narratives:
  - capability: "brand linting"
    narrative: "prevent identity drift before it ships"
    audiences: [platform_engineers]
    formats: [x_post, blog, demo_video, docs]
```

- `segments` — audience segments, each with a `name`, a `description`, and a list of `pains`.
- `narratives` — each entry maps a `capability` to a `narrative`, the `audiences` it serves (segment names), and the `formats` it ships in. This is the source of `brandi social graph` and `brandi social plan`.

## Guidelines reference

The guidelines live in `.brandi/` as three required YAML files — `voice.yaml`, `visual.yaml`, `prohibited.yaml` — plus an optional `rules.yaml`. Every field has a default, so partial files load cleanly; a missing required file is an error. `brandi init` writes the defaults shown below and never overwrites existing files.

### voice.yaml

How the brand sounds.

```yaml
traits: [precise, ambitious, human]
trait_signals:
  precise: [exactly, deterministic, verify]
  ambitious: [build, push, frontier]
  human: [you, your, we]
style:
  sentence_case_headings: true
  max_exclamation_marks: 0
  error_messages:
    forbid_prefixes: ["Error", "ERROR", "Fatal", "panic"]
    forbid_numeric_codes: true
    require_actionable: true
```

- `traits` — the voice traits of the brand.
- `trait_signals` — signal words that express each trait; used by voice and social heuristics.
- `style.sentence_case_headings` — headings use sentence case; enforced by `sentence-case-headings`.
- `style.max_exclamation_marks` — maximum exclamation marks per surface; enforced by `exclamation-limit`. The default guidelines allow none.
- `style.error_messages.forbid_prefixes` — prefixes an error message must not start with; enforced by `error-prefix`.
- `style.error_messages.forbid_numeric_codes` — forbid bare numeric codes in error messages.
- `style.error_messages.require_actionable` — error messages must tell the user what to do next; enforced by `error-actionable`.

### visual.yaml

How the brand looks.

```yaml
palette:
  primary: "#FF2DAA"
  secondary: ["#FF78C8", "#DCA7FF", "#FF4D6D", "#FFB020", "#58D5FF"]
  neutrals: ["#FFF7FC", "#160812", "#6F5B68"]
color_tolerance: 24
typography:
  style: technical_modern
  preferred_fonts: []
assets:
  social_card: { width: 1200, height: 630 }
  thumbnail: { width: 1280, height: 720 }
  social_card_path: assets/social-card.svg
  thumbnail_path: assets/thumbnail.svg
  min_whitespace: 0.25
  max_file_kb: 500
  coherence_min_palette: 50
```

- `palette.primary` — the primary brand color, `#rrggbb`.
- `palette.secondary` / `palette.neutrals` — supporting colors, `#rrggbb`.
- `color_tolerance` — how far a color may drift from a palette color (Euclidean distance across R/G/B, 0–255 scale) and still count as on-palette; enforced by `palette-adherence` and by `brandi assets check`/`audit`'s brand-colour percentage.
- `typography.style` / `typography.preferred_fonts` — recorded for humans and tools; Brandi leaves typography judgment to manual review.
- `assets.social_card.width` / `assets.social_card.height` — required social card dimensions in pixels.
- `assets.thumbnail.width` / `assets.thumbnail.height` — required thumbnail dimensions in pixels.
- `assets.social_card_path` / `assets.thumbnail_path` — project-relative `.svg` destinations used by `brandi direct`.
- `assets.min_whitespace` — minimum whitespace fraction (0.0–1.0) for image assets.
- `assets.max_file_kb` — file-size budget in KB for image assets; enforced by `brandi assets audit`.
- `assets.coherence_min_palette` — minimum brand-colour adherence percent (0–100) before `brandi assets audit` flags an asset as off-palette.

### prohibited.yaml

Language the brand forbids, with a severity per category.

```yaml
categories:
  corporate_jargon: ["synergy", "best-in-class", "seamless", "seamlessly", "cutting-edge", "world-class", "leverage our"]
  empty_hype: ["revolutionary", "game-changing", "game changer", "the future of", "awesome", "amazing", "incredible"]
  generic_ai_language: ["delve", "delving", "unlock the power", "supercharge", "elevate", "harness the", "in today's fast-paced"]
severity:
  corporate_jargon: warning
  empty_hype: error
  generic_ai_language: warning
```

- `categories` — category name mapped to a list of prohibited words and phrases; enforced by `prohibited-term`.
- `severity` — category name mapped to `error`, `warning`, or `info`, controlling the severity of the findings for that category. Unknown values are a validation error; a severity entry without a matching category is a validation warning.

### rules.yaml

Optional. Per-rule severity overrides; `brandi init` does not create this file. An absent file means every rule runs with its default severity.

```yaml
overrides:
  error-actionable: off
  sentence-case-headings: info
```

- `overrides` — rule id (see the rule catalog) mapped to `off`, `info`, `warning`, or `error`. `off` disables the rule entirely; any other value replaces the severity of its findings, which changes the score they subtract. Unknown rule ids and unknown values are validation errors reported by `brandi guidelines validate`.

## Rule catalog

Ten rules run over the scanned surfaces. Every finding carries a rule id, a severity, a location, a message, and where possible a suggestion.

| Rule id | Surfaces | Default severity | What it catches |
| --- | --- | --- | --- |
| `prohibited-term` | repo_doc, doc, ui_string | per category in `prohibited.yaml` | Words and phrases on the prohibited lists |
| `terminology-variant` | repo_doc, doc | error for banned variants and former names, warning for product-name casing | Banned terminology variants, former product names, and product-name casing drift |
| `error-prefix` | ui_string | warning | Error messages starting with a forbidden prefix or a numeric error code |
| `error-actionable` | ui_string | info | Error messages with no actionable next step |
| `exclamation-limit` | repo_doc, doc | warning | More exclamation marks than `max_exclamation_marks` allows |
| `sentence-case-headings` | repo_doc, doc | warning | Headings not written in sentence case, such as Title Case |
| `readme-mission` | repo_doc | error when README.md is missing, warning when the introduction lacks the product name or mission | README introduction missing the product name or mission |
| `readme-image-refs` | repo_doc, doc | warning | Markdown image references pointing at missing files |
| `palette-adherence` | style | warning | Hex colors outside the palette beyond `color_tolerance` |
| `readme-h1` | repo_doc | warning | README not opening with an H1 that names the product |

### Scoring and exit codes

The overall coherence score starts at 100. Finding severity is combined with detector confidence, audience exposure, and the surface's provenance-adjusted semantic weight, then normalized against the weighted corpus. Scores are also reported per surface kind. This keeps certain, public, authored product copy influential while preventing generated operational prose from dominating through volume alone. See "For gradual adoption" below.

Exit codes:

- `0` — success. For `brandi lint`: the report was produced and all gates passed.
- `1` — `brandi lint` gate failed: the overall score is below `--fail-under`, or `--strict` was set and at least one error-severity finding exists.
- `2` — runtime or usage error: bad arguments, missing brief/guidelines, unreadable files.

## Terminal UI

A companion TUI built with the Charm stack (bubbletea, lipgloss) lives in `tui/`. It wraps the `brandi` binary — every number comes from `brandi lint --format json` or the daemon state files, never from re-implemented lint logic.

```bash
cd tui
go build -o brandi-tui .
./brandi-tui --path /your/project
```

Features:

- Six views: Overview (animated coherence gauge, severity counts, per-surface bars, score-history sparkline, and the project portfolio), Findings (scrollable, grouped by file, with suggestions), Rules (the full catalog with pass/fail), Social (project-linked account sign-in, source/surface roles, content graph, plan, and ADB readiness), Tape Studio (`e` edit goal, `p` cycle presets, `n` new draft, `s` save), Growth (`m` metrics; `a`/`x` opens a second review requiring the exact ID/hash and, for approval, destination)
- Live re-lint every 2.5 s while watch mode is on (toggle with `w`), so the score moves as you edit files
- Daemon control from the keyboard: `d` starts/stops the watch daemon, with a pulsing badge while it runs
- `r` re-lints on demand, `f` rescans the project portfolio, `c` opens social sign-in, `[`/`]` selects an account, `D` twice disconnects it, `W` opens the Wi-Fi pairing wizard, `tab`/`1-6` switch views, `j`/`k` scroll, `q` quits — the footer always shows the active keybindings

Flags: `--path` (project, default `.`), `--brandi` (`brandi` binary, default: PATH then repo-local `target/` builds), `--no-watch`, `--workspace` (portfolio root for Overview).

## Daemon

`brandi daemon` watches the project tree and re-lints when surfaces change: Markdown docs, the README, source files carrying user-facing strings, and style files. File events are debounced, so a burst of writes — a save-all, a branch switch — collapses into a single re-lint.

State lives under `.brandi/state/`:

- `daemon.pid` — process id of the background daemon
- `daemon.log` — daemon log output
- `report.json` — the latest lint report, in the same shape as `brandi lint --format json`
- `history.jsonl` — one JSON object per lint run, a score history over time

The daemon excludes `.brandi/state/` from its own watch set, so writing `report.json` or appending to `history.jsonl` never re-triggers a lint.

## Development

```bash
cargo build
cargo test
```

Repository layout:

- `src/main.rs` — CLI entrypoint: parses arguments, dispatches, sets exit codes
- `src/lib.rs` — crate root and module wiring
- `src/cli.rs` — clap command-line definition
- `src/commands.rs` — one function per subcommand
- `src/error.rs` — shared error type
- `src/types.rs` — shared data types: surfaces, findings, scores, reports
- `src/brief.rs` — brief loading, validation, and scaffolding
- `src/guidelines.rs` — guidelines loading, validation, and scaffolding
- `src/surface.rs` — surface discovery and classification
- `src/propose.rs` — budgeted aesthetic revision proposals
- `src/rules.rs` — the lint rule catalog and check functions
- `src/report.rs` — report assembly and human/JSON rendering
- `src/assets.rs` — image asset checks
- `src/social.rs` — narrative graph and content planning
- `src/daemon.rs` — the watch daemon
- `tui/` — the Charm (bubbletea/lipgloss) terminal UI, a Go module wrapping the `brandi` binary

Adding a rule:

1. Add an entry to the rule catalog in `src/rules.rs`: id, surfaces, default severity, description.
2. Write the check function next to the existing ones. Return findings with a clear message and, where possible, a concrete suggestion.
3. Add unit tests in the same file.

Scoring, rendering, and the `--fail-under` / `--strict` gates pick new rules up automatically.

### CI and pre-commit

`brandi lint` exits `1` when the score drops below `--fail-under`, or when `--strict` is set and any error-severity finding exists, so it slots directly into CI:

```bash
brandi lint --fail-under 80
```

As a pre-commit hook (`.git/hooks/pre-commit`):

```sh
#!/bin/sh
brandi lint --strict
```

For gradual adoption on an existing project, start with a low gate such as `--fail-under 40` and raise it as findings are fixed, or quiet individual rules through `.brandi/rules.yaml` instead of disabling the lint entirely.
# brandi
