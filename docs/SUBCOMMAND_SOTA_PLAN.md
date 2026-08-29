# Brandi subcommand SOTA elevation plan

Status: planning draft  
Date: 2026-08-29  
Applies to: 38 executable leaf commands in `src/cli.rs`  
Parent plans: `CLI_OUTPUT_EXPERIENCE_PLAN.md`, `ENTERPRISE_ROADMAP.md`

## 1. Executive summary

This plan raises every `brandi` subcommand to enterprise-grade, state-of-the-art output quality. It treats each command as a *bounded, observable, testable service* rather than a function that prints strings. The methodology is grounded in three reasoning lenses:

- **Padagonia** — coverage-ontology modeling of commands, parameters, dimensions, constraints, renderers, events, and tests.
- **Ontism** — ontology-first reasoning about what each command is, what entities it owns, which invariants hold, and how state transitions occur.
- **Cambrian** — capability-expansion reasoning: the evolutionary pressure to diversify outputs, integrations, and intelligence while preserving safety.

Guideline reasoning and brief reasoning both use the three lenses. That means every command plan explicitly answers:

1. *Padagonia*: What parameter/equivalence-class coverage makes guideline and brief handling safe and complete?
2. *Ontism*: What are the essential brief/guideline entities, relations, and invariants for this command?
3. *Cambrian*: What new brief/guideline capabilities would move this command to SOTA?

The deliverable is a per-command specification that can be turned into implementation tasks, tests, and acceptance criteria.

## 2. Reasoning framework

### 2.1 Padagonia lens

Padagonia is the authoritative coverage graph. For each command we model:

- `LeafCommand` and its owning `CommandFamily`.
- `CommandParameter` occurrences and their semantic domains.
- `ParameterDimension` partitions (boolean, enum, numeric, path, string, secret, confirmation).
- `Constraint` nodes that exclude unsafe or meaningless combinations.
- `OutputEvent` nodes the command may/must emit.
- `Renderer` mappings (human, plain, JSON, JSONL, Barbara).
- `CoverageRequirement` and `TestCase` edges.

For brief/guideline reasoning, Padagonia forces us to enumerate equivalence classes of input files: valid full brief, partial brief, missing files, invalid YAML, empty collections, banned variants, unknown severities, malformed palette colors, etc. Every class must be covered or explicitly excepted with evidence.

### 2.2 Ontism lens

Ontism asks what a command *is* in the brand ontology:

- **Entities**: brief (`identity.yaml`, `audience.yaml`), guidelines (`voice.yaml`, `visual.yaml`, `prohibited.yaml`, `rules.yaml`), surfaces, findings, assets, drafts, devices, queues.
- **Relations**: a command *loads*, *validates*, *scans*, *lints*, *generates*, *approves*, *publishes*, or *watches* these entities.
- **Invariants**: e.g., a lint command never mutates project files; an approval command never changes state without an exact confirmation; a daemon never writes into `.brandi/state/` without appending an audit record.
- **State transitions**: e.g., draft `pending` → `approved` → `staged` → `published`, with each transition gated by role, destination, expiry, and confirmation.

For brief/guideline reasoning, ontism requires us to state the exact schema versions, default values, validation rules, and semantic mappings that a command depends on.

### 2.3 Cambrian lens

Cambrian reasoning asks how the command evolves toward SOTA:

- What richer inputs can it accept (multilingual briefs, design-system tokens, competitor tone samples)?
- What richer outputs can it produce (semantic JSONL streams, structured receipts, diff-style proposals, embedded rationale)?
- What integrations can it safely support (CI annotations, SARIF, GitHub Checks, Kaptaind webhooks, Figma, Canva)?
- What intelligence can it add without hallucinating (local embedding similarity, deterministic style transfer, evidence-bound revision)?

Cambrian expansion is constrained by Padagonia coverage and Ontism invariants: new capabilities must be modeled, tested, and bounded before release.

## 3. Command-family overview

| Family | Commands | Primary entities | Output modes | Safety class |
|---|---|---|---|---|
| Asset generation | `direct`, `reshoot` | brief, guidelines, assets, plans, evidence | human, JSON | mutation/external |
| Scaffolding | `init` | `.brandi/` files | human, JSON | local mutation |
| Validation | `brief validate`, `guidelines validate` | brief/guidelines YAML | human, JSON | read-only |
| Scan/lint | `scan`, `propose`, `check`, `lint`, `evaluate` | surfaces, findings, rules, corpus | human, JSON, JSONL | read-only |
| Assets | `assets check`, `assets list`, `assets audit` | image files, palette, references | human, JSON | read-only |
| Social | `social graph`, `social plan` | brief narratives, segments, formats | human, JSON | read-only |
| ADB social | `social adb status`, `stage`, `publish`, `wifi pair/connect/disconnect/bridge` | devices, targets, drafts, deliveries | human, JSON | external mutation |
| Daemon | `daemon run`, `start`, `stop`, `status` | process, state, reports, history | human, JSONL | background mutation |
| Tape | `tape generate`, `validate`, `render` | tape drafts, VHS, sandbox | human, JSON | generated execution |
| Promotion | `promotion plan`, `stats`, `sync`, `milestones`, `queue`, `approve`, `reject` | queue, outbox, milestones, metrics | human, JSON | external mutation |
| Telegram | `telegram run`, `status` | bot config, projects, routes | human, JSONL | external mutation |

## 4. Per-family SOTA plans

### 4.1 Asset generation family

#### `brandi direct`

**Current state.** Generates missing brand assets from the brief/guidelines. Supports pre-flight mode, portfolio mode, section selection, provider routes, reviewer, concurrency/cost caps, Kaptaind post-flight, and source-image evidence. Outputs a per-project report.

**Padagonia reasoning.**
- Parameters: `path`, `portfolio`, `section`, `runs`, `strategy`, `routes`, `reviewer`, `max_concurrency`, `max_cost_usd`, `sources`, `preflight_only`, `kaptaind`, `confirm_postflight`.
- Equivalence classes: single project vs portfolio; identity/product/audience sections; zero/one/many routes; preflight vs generation; cost cap exceeded; source image missing/unreadable; Kaptaind modes `none`/`analysis`/`aim`/`push`.
- Events: `CommandStarted`, `PhaseStarted` (plan, generate, review, post-flight), `Progressed`, `Preview` (pre-flight plan), `StateChanged` (queue draft created), `CommandFinished`.
- Coverage requirement: every portfolio project failure is isolated and reported; post-flight confirmation replay is rejected if stale.

**Ontism reasoning.**
- Entities: `DirectProjectReport`, `DirectOutcome`, `DirectPreflight`, `GenerationRoute`, `ImmutablePlan`.
- Invariants: existing assets are never overwritten; provider cost never exceeds the cap; post-flight mutates external state only after exact confirmation; portfolio failures do not abort the whole run.
- Brief/guideline dependency: `identity.product.name`, `visual.palette`, `visual.typography`, `visual.assets.*` are authoritative. Missing required fields must downgrade gracefully to defaults or emit a validation finding.

**Cambrian reasoning.**
- SOTA capability: generate full asset *families* (social card, thumbnail, icon set, favicon, README hero) in one coherent run.
- SOTA capability: accept a `competitor_moodboard` or `reference_url` as brief evidence and constrain generation by similarity.
- SOTA capability: emit deterministic SVG + RUGID scene bundles that can be re-rendered without a provider.
- SOTA capability: integrate with `direct --ci` to fail when required assets are missing.

**Output experience.**
- Human: structured project cards with plan ID, strategy, actions, cost, palette, file paths, and post-flight token.
- JSON: stable `brandi-direct-v1` schema including per-project errors.
- Motion: phase-level progress for generation/review; static pre-flight plan.
- Safety: pre-flight preview must show exact files, estimated cost, and confirmation token before any provider call or Kaptaind action.

**Acceptance.**
- Portfolio run with one failing project returns exit 1 and a per-project summary.
- Preflight output is identical regardless of provider availability.
- Cost cap is enforced before the provider call that would exceed it.
- Post-flight confirmation replay with a stale token is rejected.

#### `brandi reshoot`

**Current state.** Revises every discovered image, stylesheet colour token, and non-code documentation surface against the current brief/guidelines. Excludes source code strings and generated files.

**Padagonia reasoning.**
- Parameters: `path`, `portfolio`.
- Equivalence classes: zero assets to revise; raster vs SVG vs CSS token vs Markdown prose; portfolio bounded to 6 levels; project fails validation but continues.
- Events: `CommandStarted`, `PhaseStarted` (discover, classify, revise, audit), `Progressed`, `StateChanged` (file revision queued/applied), `CommandFinished`.
- Coverage requirement: verify that no source-code file, dependency, or `.brandi/` config file is modified.

**Ontism reasoning.**
- Entities: `ReshootSurface`, `ReshootRevision`, `ReshootReport`.
- Invariants: revisions are dry-run by default unless an explicit `--apply` confirmation is added; source-code strings are excluded by surface kind, not filename guessing; raster colour mapping uses the configured `color_tolerance`.
- Brief/guideline dependency: `visual.palette` and `visual.assets` drive colour mapping and dimension checks; `voice.traits` drive prose revision tone.

**Cambrian reasoning.**
- SOTA capability: generate a `reshoot --plan` that emits a patch file and confirmation token before applying any change.
- SOTA capability: semantic segmentation of images to preserve logo/text regions during palette remapping.
- SOTA capability: prose revision with per-sentence rationale citing the guideline rule.
- SOTA capability: integration with `git apply --check` to preview merge conflicts.

**Output experience.**
- Human: per-surface before/after preview with byte deltas and confidence scores.
- JSON: stable `brandi-reshoot-v1` schema with `changed`, `skipped`, `failed` arrays.
- Safety: default dry-run; apply mode requires exact confirmation and logs to security audit.

**Acceptance.**
- Reshoot without `--apply` does not modify any file.
- Source code files are absent from the changed list.
- Portfolio failure isolation matches `direct`.

---

### 4.2 Scaffolding family

#### `brandi init`

**Current state.** Scaffolds `.brandi/` with default brief and guidelines files and an `assets/` folder. Idempotent — existing files are never overwritten.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: empty directory; existing `.brandi/` with partial files; read-only parent; nested project inside portfolio.
- Events: `CommandStarted`, `StateChanged` (file created), `CommandFinished`.
- Coverage requirement: idempotency must be machine-verifiable; created files must pass `brief validate` and `guidelines validate` immediately.

**Ontism reasoning.**
- Entities: `Brief`, `Guidelines`, `ScaffoldRecord`.
- Invariants: existing files are never overwritten; defaults are valid; generated files carry a schema version comment.
- Brief/guideline reasoning: the default brief and guidelines are themselves a coherent brand definition for Brandi. They must be kept aligned with this repository's actual README and voice.

**Cambrian reasoning.**
- SOTA capability: interactive wizard `--interactive` that asks product name, audience, tone, and primary colour, then writes tailored defaults.
- SOTA capability: `--template <name>` to scaffold for open-source library, SaaS, design system, or personal brand.
- SOTA capability: emit a `.brandiignore` tuned to the detected project type (Rust, Go, Node, Python).

**Output experience.**
- Human: "created N files" with relative paths; "already exists, nothing to do" when idempotent.
- JSON: `brandi-init-v1` schema with `created` and `changed`.
- Safety: never overwrite; report permission errors with a clear action.

**Acceptance.**
- `brandi init && brandi brief validate && brandi guidelines validate` exits 0 on a fresh directory.
- Re-running `brandi init` does not change existing files.

---

### 4.3 Validation family

#### `brandi brief validate`

**Current state.** Validates `identity.yaml` and `audience.yaml`. Prints `brief OK`, warnings, or problems.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: valid full brief; partial files with defaults; missing file; invalid YAML; empty product name; no audience segments; duplicate segment names; narrative references unknown segment.
- Events: `CommandStarted`, `Diagnostic` (per warning/problem), `CommandFinished`.
- Coverage requirement: hard failures must emit a stable `brandi-validation-v1` JSON document with `valid: false`.

**Ontism reasoning.**
- Entities: `Brief`, `Identity`, `Audience`, `Segment`, `Narrative`.
- Invariants: a missing required file is a hard failure; defaults are always valid; warnings do not affect exit code 0; every problem points to a file and a semantic field.
- Brief reasoning: the brief is the brand's constitution. Validation must verify internal consistency (e.g., every narrative audience exists as a segment).

**Cambrian reasoning.**
- SOTA capability: validate that the brief is *lived* by the project — e.g., README mission matches `product.mission`, social plan references real files.
- SOTA capability: suggest audience segment refinements based on narrative coverage gaps.
- SOTA capability: cross-lingual brief validation (canonical terminology per locale).

**Output experience.**
- Human: one problem per line with file/field context; "brief OK" when clean.
- JSON: `brandi-validation-v1` with `subject`, `valid`, `warnings`, `problems`.
- Safety: problems are sorted deterministically by file and line.

**Acceptance.**
- Invalid YAML returns exit 2 with a machine-readable problem.
- A brief with only default values passes with zero warnings.

#### `brandi guidelines validate`

**Current state.** Validates `voice.yaml`, `visual.yaml`, `prohibited.yaml`, and optional `rules.yaml`.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: valid guidelines; malformed palette color; unknown severity; severity without category; unknown rule id in `rules.yaml`; empty prohibited list; invalid color tolerance.
- Events: same as brief validate.
- Coverage requirement: every malformed color grammar is caught; every unknown rule id is reported.

**Ontism reasoning.**
- Entities: `Guidelines`, `Voice`, `Visual`, `Prohibited`, `RuleOverrides`.
- Invariants: palette colors are parseable `#rrggbb`; severities are `error|warning|info`; rule overrides reference known rule ids; `color_tolerance` is in 0..255.
- Guideline reasoning: guidelines are the enforceable law. Validation must prove the law is internally consistent before it is applied.

**Cambrian reasoning.**
- SOTA capability: validate that every prohibited term has a suggested replacement.
- SOTA capability: detect palette accessibility issues (contrast, color-blindness safety).
- SOTA capability: validate that voice trait signals are present in actual project copy.

**Output experience.**
- Same shape as `brief validate` for consistency.

**Acceptance.**
- `#ggg` returns a problem pointing to the exact line.
- A severity entry without a matching category returns a warning, not a hard failure.

---

### 4.4 Scan/lint family

#### `brandi scan`

**Current state.** Walks the project tree, classifies user-facing surfaces, and prints per-kind counts and a capped list.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: empty project; only `.brandi/`; large monorepo; unreadable paths; `.brandiignore` exclusions; generated/fixture directories.
- Events: `CommandStarted`, `PhaseStarted` (walk, classify), `Progressed`, `Item` (per surface), `Diagnostic` (per scan diagnostic), `CommandFinished`.
- Coverage requirement: every skipped file has a reason; completeness flag is accurate.

**Ontism reasoning.**
- Entities: `Surface`, `SurfaceKind`, `ScanDiagnostic`, `ScanResult`.
- Invariants: scan is read-only; `.brandi/state/` is excluded; `.brandiignore` is respected; surface kinds are mutually exclusive and deterministic.
- Brief/guideline reasoning: scan surfaces are the *domain* over which brief and guidelines are applied. The ontology of surfaces (repo_doc, doc, ui_string, style) must be versioned and stable.

**Cambrian reasoning.**
- SOTA capability: AST-aware extraction for Rust, Go, JS/TS, Python with source-span preservation.
- SOTA capability: classify localization strings, template strings, log strings separately.
- SOTA capability: export scan surfaces as a SARIF-like artifact for CI.

**Output experience.**
- Human: total count, per-kind bars, capped surface list with confidence/exposure, diagnostics.
- JSON: `brandi-scan-v1` schema.
- Motion: delayed spinner for large trees; progress events when total is known.

**Acceptance.**
- Scan of a project with only generated files returns zero surfaces and `complete: true`.
- Unreadable directory is reported as a diagnostic, not a panic.

#### `brandi propose`

**Current state.** Scans stylisable surfaces and prints budgeted before/after revisions.

**Padagonia reasoning.**
- Parameters: `path`, `budget`.
- Equivalence classes: zero proposals; budget 0; budget 12; more candidates than budget; no valid brief/guidelines.
- Events: `CommandStarted`, `Progressed`, `Item` (per proposal), `CommandFinished`.
- Coverage requirement: budget is strictly enforced; proposals cite a rule and a guideline source.

**Ontism reasoning.**
- Entities: `Proposal`, `ProposalReason`, `StylisableSurface`.
- Invariants: propose is a dry run; every proposal maps to a guideline rule or palette value; banned variants are replaced with canonical terms.
- Guideline reasoning: proposals derive their authority from `prohibited.yaml`, `voice.yaml`, and `visual.yaml`. A proposal without a guideline citation is invalid.

**Cambrian reasoning.**
- SOTA capability: rank proposals by expected coherence improvement, not just rule hits.
- SOTA capability: generate multi-sentence revisions that preserve Markdown structure and code fences.
- SOTA capability: `--apply` mode with patch generation and confirmation.

**Output experience.**
- Human: numbered list with location, reason, current text, revised text.
- JSON: `brandi-proposals-v1` with `budget`, `generated`, `proposals`.
- Safety: never write files; cap output list.

**Acceptance.**
- `--budget 5` returns at most 5 proposals.
- Every proposal references a rule id and a guideline field.

#### `brandi check <FILE>`

**Current state.** Lints a single file and prints a human report.

**Padagonia reasoning.**
- Parameters: `file`, `path`.
- Equivalence classes: file inside project; file outside project; missing file; unreadable file; file excluded by `.brandiignore`; empty file.
- Events: `CommandStarted`, `Diagnostic` (per finding), `CommandFinished`.
- Coverage requirement: identical rule set as `lint`; per-file baseline support.

**Ontism reasoning.**
- Entities: `FileFinding`, `FileReport`.
- Invariants: a file outside the project root is rejected; findings include rule id, severity, line, column, message, suggestion.
- Brief/guideline reasoning: `check` applies the same brief/guidelines as `lint` but to a single surface. The rule engine must not special-case single files.

**Cambrian reasoning.**
- SOTA capability: `--fix-dry-run` to preview fixes.
- SOTA capability: JSON diff output for CI review comments.
- SOTA capability: inline GitHub suggestion format.

**Output experience.**
- Human: same report style as `lint` but scoped to one file.
- JSON: same schema as `lint` report.

**Acceptance.**
- `check README.md` and `lint` produce consistent findings for the same file.
- Missing file returns exit 2 with a clear code.

#### `brandi lint`

**Current state.** Lints the whole project, produces scores, supports `--fail-under`, `--strict`, and baseline delta.

**Padagonia reasoning.**
- Parameters: `path`, `fail_under`, `strict`, `baseline`.
- Equivalence classes: clean project; errors only; warnings only; incomplete scan; score exactly at threshold; baseline introduces new findings.
- Events: `CommandStarted`, `PhaseStarted` (load, scan, evaluate), `Progressed`, `Diagnostic`, `CommandFinished`.
- Coverage requirement: exit-code matrix is stable across all format/policy combinations.

**Ontism reasoning.**
- Entities: `Report`, `Scores`, `BaselineDelta`, `Finding`.
- Invariants: score starts at 100 and subtracts per severity; incomplete scan cannot score 100; baseline delta flags only new findings, not resolved ones; `--strict` exits 1 on any error.
- Guideline reasoning: scoring model is a function of guidelines severities and rule overrides. The model version must be recorded in the report.

**Cambrian reasoning.**
- SOTA capability: density-normalized scores for monorepos.
- SOTA capability: SARIF and GitHub Checks output formats.
- SOTA capability: rule-level precision/recall annotations from the evaluation corpus.
- SOTA capability: trend analysis against daemon history.

**Output experience.**
- Human: header, surface counts, findings grouped by file, passed rules, score bar.
- JSON: `brandi-lint-v1` schema.
- Motion: delayed spinner; final report is static.

**Acceptance.**
- `--fail-under 80` exits 1 when overall score is 79.
- `--strict` exits 1 when any error exists even if score is above threshold.
- Baseline delta correctly identifies new findings only.

#### `brandi evaluate`

**Current state.** Evaluates extractor precision/recall against a versioned corpus.

**Padagonia reasoning.**
- Parameters: `corpus`, `fail_under_precision`, `fail_under_recall`.
- Equivalence classes: corpus missing; empty corpus; precision below threshold; recall below threshold; malformed labels.
- Events: `CommandStarted`, `Progressed`, `Diagnostic`, `CommandFinished`.
- Coverage requirement: every labeled surface is scored; confusion matrix is emitted.

**Ontism reasoning.**
- Entities: `Corpus`, `LabeledSurface`, `EvaluationResult`, `ConfusionMatrix`.
- Invariants: evaluation is read-only; corpus schema version is checked; false positives and false negatives are itemized.
- Brief/guideline reasoning: evaluation validates that the *brief/guideline application layer* is correct, not just the extractor.

**Cambrian reasoning.**
- SOTA capability: per-language precision/recall breakdown.
- SOTA capability: automated corpus expansion from open-source brand-coherence examples.
- SOTA capability: statistical significance tests and regression alerts.

**Output experience.**
- Human: precision/recall/F1, confusion matrix, top error clusters.
- JSON: `brandi-evaluation-v1` schema.

**Acceptance.**
- Returns exit 1 when precision is below threshold.
- Malformed corpus returns exit 2 with the first invalid record location.

---

### 4.5 Assets family

#### `brandi assets check <IMAGES...>`

**Current state.** Checks image assets against the guidelines' asset spec. Supports `--kind auto|social-card|thumbnail|icon`.

**Padagonia reasoning.**
- Parameters: `images`, `kind`, `path`.
- Equivalence classes: one image; many images; missing image; invalid image; kind auto-inference; wrong dimensions; off-palette; low whitespace.
- Events: `CommandStarted`, `Item` (per asset), `Diagnostic`, `CommandFinished`.
- Coverage requirement: auto-inference matches explicit kind; SVG and raster are handled.

**Ontism reasoning.**
- Entities: `Asset`, `AssetKind`, `AssetCheckResult`, `Palette`.
- Invariants: every checked file is classified; dimensions, palette adherence, whitespace, and file size are measured; manual-review items are explicit.
- Guideline reasoning: `visual.assets` defines expected dimensions and budgets; `visual.palette` and `color_tolerance` define palette adherence.

**Cambrian reasoning.**
- SOTA capability: detect typography issues (text too small, missing alt text).
- SOTA capability: detect composition balance and safe zones.
- SOTA capability: generate a corrected asset when deviations are found.

**Output experience.**
- Human: per-asset table with dimensions, brand colours, whitespace, dominant colours, manual review checklist.
- JSON: `brandi-asset-check-v1` schema.

**Acceptance.**
- A 1200x630 social card matching the palette passes all checks.
- A missing image returns a structured error, not a panic.

#### `brandi assets list`

**Current state.** Lists discovered brand assets.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: no assets; assets in `assets/`; assets referenced by README; assets with ambiguous names.
- Events: `CommandStarted`, `Item`, `CommandFinished`.

**Ontism reasoning.**
- Entities: `Asset`, `AssetReference`.
- Invariants: listing is read-only; classification matches `assets check --kind auto`.

**Cambrian reasoning.**
- SOTA capability: include reference counts and where each asset is used.
- SOTA capability: flag orphan assets (present but unreferenced).

**Output experience.**
- Human: path, inferred kind, dimensions, size.
- JSON: `brandi-asset-list-v1`.

**Acceptance.**
- Output classification is consistent with `assets check --kind auto`.

#### `brandi assets audit`

**Current state.** Project-wide asset coherence audit: duplicates, icon-set drift, file-size budget, reference counts.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: zero assets; duplicates; near-duplicates; oversized files; mixed icon styles.
- Events: `CommandStarted`, `PhaseStarted`, `Progressed`, `Diagnostic`, `CommandFinished`.

**Ontism reasoning.**
- Entities: `AssetAudit`, `DuplicateGroup`, `ReferenceCount`.
- Invariants: score is 100 minus penalties, saturating at 0; every issue has a path and a reason.
- Guideline reasoning: `visual.assets.max_file_kb` and `visual.assets.coherence_min_palette` are enforced.

**Cambrian reasoning.**
- SOTA capability: near-duplicate detection with perceptual hashing.
- SOTA capability: automated asset cleanup plan with confirmation.
- SOTA capability: accessibility audit (alt text, contrast).

**Output experience.**
- Human: score, issue list with paths, reference summary.
- JSON: `brandi-asset-audit-v1`.

**Acceptance.**
- Two identical files are grouped as duplicates.
- Score cannot exceed 100 or be negative.

---

### 4.6 Social family

#### `brandi social graph`

**Current state.** Renders the narrative graph: capabilities → narratives → audiences → formats.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: no narratives; disconnected segments; single audience; multiple formats.
- Events: `CommandStarted`, `Item` (nodes/edges), `CommandFinished`.

**Ontism reasoning.**
- Entities: `NarrativeGraph`, `CapabilityNode`, `AudienceNode`, `FormatNode`.
- Invariants: every narrative references valid segments; orphan segments are reported; graph is acyclic.
- Brief reasoning: the graph is derived entirely from `audience.yaml`. It is a *view* of the brief, not a mutation.

**Cambrian reasoning.**
- SOTA capability: render as Mermaid/Graphviz/DOT.
- SOTA capability: detect narrative gaps (capabilities with no format, audiences with no narrative).
- SOTA capability: estimate content velocity per format.

**Output experience.**
- Human: indented tree or Graphviz.
- JSON: `brandi-social-graph-v1`.

**Acceptance.**
- Invalid segment reference in a narrative returns a validation-style error.

#### `brandi social plan [--segment NAME]`

**Current state.** Renders a content plan derived from the brief.

**Padagonia reasoning.**
- Parameters: `path`, `segment`.
- Equivalence classes: no segment (all); valid segment; unknown segment; segment with no narratives.
- Events: `CommandStarted`, `Item` (per plan item), `CommandFinished`.

**Ontism reasoning.**
- Entities: `ContentPlan`, `PlanItem`.
- Invariants: every plan item maps to a narrative; unknown segment returns exit 2.
- Brief reasoning: the plan is evidence-bound to capabilities and audience pains.

**Cambrian reasoning.**
- SOTA capability: generate a calendar/schedule with suggested publish dates.
- SOTA capability: produce platform-specific copy variants (X, LinkedIn, blog).
- SOTA capability: tie plan items to Kaptaind milestones.

**Output experience.**
- Human: segment-filtered plan with capability, narrative, audience, format, and key messages.
- JSON: `brandi-social-plan-v1`.

**Acceptance.**
- `--segment unknown` exits 2 with a list of valid segments.

---

### 4.7 ADB social family

#### `brandi social adb status`

**Current state.** Lists configured targets and connected devices without changing state.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: adb missing; no devices; one device; multiple devices; no targets configured; invalid config.
- Events: `CommandStarted`, `Item`, `Diagnostic`, `CommandFinished`.
- Safety class: read-only external probe.

**Ontism reasoning.**
- Entities: `AdbStatus`, `Device`, `Target`.
- Invariants: status check never runs `adb` with mutating arguments; secrets are not echoed.

**Cambrian reasoning.**
- SOTA capability: cache device list with TTL; detect device state changes.
- SOTA capability: show screenshot thumbnail and app version for each target.

**Output experience.**
- Human: adb readiness, device count, target count, per-device details.
- JSON: `brandi-adb-status-v1`.

**Acceptance.**
- Status returns 0 even when adb is missing, reporting the fact.

#### `brandi social adb wifi pair`

**Current state.** Pairs Android wireless debugging and connects the debug endpoint.

**Padagonia reasoning.**
- Parameters: `pair_endpoint`, `connect_endpoint`, `code`, `code_env`.
- Equivalence classes: code from env; code from flag; missing code; wrong code; timeout.
- Events: `CommandStarted`, `Preview` (never shows code), `StateChanged`, `CommandFinished`.
- Safety: pairing code is never rendered or logged.

**Ontism reasoning.**
- Entities: `WifiConnection`, `PairingCode`.
- Invariants: code is read from environment by default; flag usage emits a warning; code is cleared from memory promptly; command arrays, not shell strings.

**Cambrian reasoning.**
- SOTA capability: QR-code pairing support.
- SOTA capability: automatic endpoint discovery via mDNS.

**Output experience.**
- Human: action, endpoint, result message; no code echoed.
- JSON: connection result.

**Acceptance.**
- Passing `--code` emits a warning on stderr.
- Pairing code does not appear in stdout, stderr, logs, or events.

#### `brandi social adb wifi connect/disconnect/bridge`

**Current state.** Manages Wi-Fi ADB connections.

**Padagonia reasoning.**
- Parameters: `endpoint` (connect/disconnect); `device`, `host`, `port` (bridge).
- Equivalence classes: already connected; unknown endpoint; bridge with missing USB device.
- Events: `CommandStarted`, `StateChanged`, `CommandFinished`.

**Ontism reasoning.**
- Entities: `WifiConnection`.
- Invariants: disconnect only affects the explicit endpoint; bridge requires USB-online device.

**Cambrian reasoning.**
- SOTA capability: connection health check with reconnect.
- SOTA capability: encrypted ADB-over-SSH tunnel.

**Output experience.**
- Human: action/endpoint/result.
- JSON: connection result.

**Acceptance.**
- Disconnect of unknown endpoint returns exit 0 with a clear message.

#### `brandi social adb stage <ID>`

**Current state.** Opens an approved draft in an allowlisted Android app composer.

**Padagonia reasoning.**
- Parameters: `id`, `target`, `device`, `path`.
- Equivalence classes: draft approved for `adb:x`; draft not approved; draft expired; target not allowlisted; device offline.
- Events: `CommandStarted`, `Preview`, `StateChanged` (staged), `CommandFinished`.
- Safety: stage opens composer but never taps publish.

**Ontism reasoning.**
- Entities: `Delivery`, `Draft`, `Target`, `Identity`.
- Invariants: requires `DeviceOperator` role; draft must be approved for exact destination; content hash is checked; staging is logged to security audit.

**Cambrian reasoning.**
- SOTA capability: preview the rendered post before staging.
- SOTA capability: support multiple target apps with per-app intent schema.

**Output experience.**
- Human: staged draft id, target, device, content preview.
- JSON: delivery receipt.

**Acceptance.**
- Unapproved draft is rejected before any ADB call.
- Expired approval is rejected.

#### `brandi social adb publish <ID>`

**Current state.** Stages and taps the configured publish control after exact confirmation.

**Padagonia reasoning.**
- Parameters: `id`, `target`, `device`, `confirm`, `path`.
- Equivalence classes: correct confirmation; wrong confirmation; confirmation bound to wrong draft; already published.
- Events: `CommandStarted`, `Preview`, `StateChanged` (published), `CommandFinished`.
- Safety: publish requires `--confirm` equal to the draft ID exactly; requires both `DeviceOperator` and `Publisher` roles.

**Ontism reasoning.**
- Entities: `Delivery`, `PublishedRevision`.
- Invariants: exact draft-ID confirmation; revalidation of approval, expiry, destination, roles; immutable audit record.

**Cambrian reasoning.**
- SOTA capability: two-person rule (separate approver and publisher).
- SOTA capability: scheduled publish with timezone-aware window.

**Output experience.**
- Human: published draft id, target, device, content hash.
- JSON: delivery receipt.

**Acceptance.**
- Wrong confirmation token is rejected.
- Replay of a published draft is rejected.

---

### 4.8 Daemon family

#### `brandi daemon run`

**Current state.** Runs the watch daemon in the foreground.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: already running; no `.brandi/`; foreground output is unbounded.
- Events: `CommandStarted`, `StateChanged` (started), lifecycle events, `CommandFinished`.
- Safety: rejects `--format json` because output is unbounded.

**Ontism reasoning.**
- Entities: `Daemon`, `WatchSet`, `Report`, `History`.
- Invariants: daemon excludes `.brandi/state/` from watch; debounces file events; writes `report.json` and `history.jsonl` atomically.

**Cambrian reasoning.**
- SOTA capability: health endpoint and metrics.
- SOTA capability: incremental lint with content hashing.
- SOTA capability: webhook notifications on score regression.

**Output experience.**
- Human: lifecycle log events; no spinner.
- JSONL: structured lifecycle and report events.

**Acceptance.**
- `--format json` returns exit 2 with a clear message.
- Writing `report.json` does not trigger a re-lint.

#### `brandi daemon start/stop/status`

**Current state.** Background daemon lifecycle.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: start when running; stop when not running; stale PID file; status clean/failing.
- Events: `CommandStarted`, `StateChanged`, `CommandFinished`.

**Ontism reasoning.**
- Entities: `DaemonProcess`, `PidFile`.
- Invariants: start writes PID atomically; stop verifies PID fingerprint; status distinguishes running/stopped/corrupt.

**Cambrian reasoning.**
- SOTA capability: systemd/launchd service files.
- SOTA capability: daemon socket for TUI to avoid polling.

**Output experience.**
- Human: PID, status, last run time, score.
- JSON: `brandi-daemon-status-v1`.

**Acceptance.**
- Stale PID is detected and reported.
- Stop of stopped daemon returns exit 0 with an informative message.

---

### 4.9 Tape family

#### `brandi tape generate`

**Current state.** Generates a structured tape draft; writes only when `--output` is supplied.

**Padagonia reasoning.**
- Parameters: `path`, `goal`, `preset`, `slug`, `output`, `force`.
- Equivalence classes: no provider; deterministic preset; output already exists without `--force`.
- Events: `CommandStarted`, `PhaseStarted`, `Progressed`, `Item`, `CommandFinished`.

**Ontism reasoning.**
- Entities: `TapeDraft`, `TapeRequest`, `TapePlan`.
- Invariants: generation is read-only unless `--output` is set; existing file is not overwritten without `--force`; scenes are typed `program` + `args`.

**Cambrian reasoning.**
- SOTA capability: generate tape from lint findings ("show how to fix X").
- SOTA capability: multi-scene narrative with voiceover cues.

**Output experience.**
- Human: plan title, provider, tape body.
- JSON: `brandi-tape-draft-v1`.

**Acceptance.**
- Generation without `--output` does not create files.
- Existing output without `--force` returns exit 2.

#### `brandi tape validate`

**Current state.** Runs deterministic safety checks and native `vhs validate`.

**Padagonia reasoning.**
- Parameters: `file`, `path`.
- Equivalence classes: valid tape; forbidden shell; unknown command; path escape; oversized output.
- Events: `CommandStarted`, `Diagnostic`, `CommandFinished`.

**Ontism reasoning.**
- Entities: `TapeReport`, `TapeIssue`.
- Invariants: only allowlisted commands with validated arguments pass; every issue has a level, code, and message.

**Cambrian reasoning.**
- SOTA capability: validate against a sandbox policy, not just syntax.
- SOTA capability: auto-fix common issues.

**Output experience.**
- Human: valid/invalid, issue list.
- JSON: `brandi-tape-validation-v1`.

**Acceptance.**
- A tape containing `rm -rf /` is rejected.
- Native `vhs validate` failure is surfaced as an issue.

#### `brandi tape render`

**Current state.** Renders a validated tape with Bubblewrap sandbox after exact confirmation.

**Padagonia reasoning.**
- Parameters: `file`, `path`, `confirm`.
- Equivalence classes: missing confirmation; wrong confirmation; sandbox unavailable; render timeout.
- Events: `CommandStarted`, `Preview`, `StateChanged`, `CommandFinished`.
- Safety: requires SHA-256 confirmation; sandbox with no network/host credentials; read-only source mount.

**Ontism reasoning.**
- Entities: `TapePreview`, `TapeExecution`, `SandboxPolicy`.
- Invariants: confirmation token matches the previewed plan; execution is recorded with plan hash, approver, sandbox digest, timestamps, and result.

**Cambrian reasoning.**
- SOTA capability: render to multiple formats (GIF, MP4, WebM, APNG).
- SOTA capability: transcript generation and subtitle burn-in.

**Output experience.**
- Human: preview first; render result with output path.
- JSON: preview or execution record.

**Acceptance.**
- Render without confirmation shows preview and token.
- Wrong confirmation is rejected.
- Sandbox failure does not leak host credentials.

---

### 4.10 Promotion family

#### `promotion plan`, `promotion stats`, `promotion sync`, `promotion milestones`, `promotion queue`

**Current state.**
- `plan`: dynamic promotion plan.
- `stats`: Git/GitHub metric snapshots.
- `sync`: replay Padagonia outbox.
- `milestones`: ingest Kaptaind release indexes.
- `queue`: list approval queue.

**Padagonia reasoning.**
- Parameters: `path`.
- Equivalence classes: empty queue; no Kaptaind index; outbox with failures; Git repo absent.
- Events: `CommandStarted`, `PhaseStarted`, `Progressed`, `Item`, `Diagnostic`, `CommandFinished`.

**Ontism reasoning.**
- Entities: `PromotionPlan`, `MetricSnapshot`, `PadagoniaRecord`, `MilestoneDraft`, `QueueEntry`.
- Invariants: `sync` is idempotent; failed records are retained; milestones are deduplicated by ID.

**Cambrian reasoning.**
- SOTA capability: multi-channel plans (Telegram, X, LinkedIn, blog, email).
- SOTA capability: trend-aware stats with anomaly detection.
- SOTA capability: automatic milestone drafting from conventional commits.

**Output experience.**
- Human: compact summaries with counts and IDs.
- JSON: stable schemas per subcommand.

**Acceptance.**
- `sync` replay of a failed record does not lose it on second failure.
- `milestones` is idempotent across repeated runs.

#### `promotion approve/reject <ID>`

**Current state.** Approves or rejects a queued revision with exact confirmation.

**Padagonia reasoning.**
- Parameters: `id`, `path`, `confirm`, `destination` (approve), `reason` (reject).
- Equivalence classes: correct confirmation; wrong confirmation; stale/expired approval; cross-destination attempt; missing role.
- Events: `CommandStarted`, `Preview`, `StateChanged`, `CommandFinished`.
- Safety: exact draft-ID confirmation; role `Approver`; destination binding; expiry.

**Ontism reasoning.**
- Entities: `QueueEntry`, `Approval`, `Rejection`, `Identity`.
- Invariants: approval binds to one destination for 15 minutes; rejection records reason; both log to security audit.

**Cambrian reasoning.**
- SOTA capability: multi-signature approval for high-impact publications.
- SOTA capability: approval policy engine (e.g., no publish on weekends).

**Output experience.**
- Human: draft ID and new status.
- JSON: updated draft.

**Acceptance.**
- Approval with wrong confirmation is rejected.
- Approved draft cannot be approved for a different destination without re-approval.

---

### 4.11 Telegram family

#### `brandi telegram run`

**Current state.** Runs hub or fleet supervisor in foreground.

**Padagonia reasoning.**
- Parameters: `config`.
- Equivalence classes: hub mode; fleet mode; public mode enabled/disabled; empty allowlist.
- Events: `CommandStarted`, lifecycle events, `CommandFinished`.
- Safety: rejects `--format json`; deny-by-default allowlist.

**Ontism reasoning.**
- Entities: `TelegramConfig`, `Bot`, `ProjectRoute`, `Role`.
- Invariants: empty allowlist rejected unless `public_mode: true`; read-only commands need viewer role; approval/publication need separate roles.

**Cambrian reasoning.**
- SOTA capability: inline keyboard previews and confirmations.
- SOTA capability: rate limiting and spam detection.

**Output experience.**
- Human: startup message with mode and route count.
- JSONL: structured bot events.

**Acceptance.**
- Empty allowlist without public mode fails at startup.
- Public mode emits a startup warning.

#### `brandi telegram status`

**Current state.** Validates config and reports worker readiness.

**Padagonia reasoning.**
- Parameters: `config`.
- Equivalence classes: valid config; missing config; invalid YAML; missing token variable.
- Events: `CommandStarted`, `Diagnostic`, `CommandFinished`.

**Ontism reasoning.**
- Entities: `TelegramConfig`, `ValidationResult`.
- Invariants: config is validated without contacting Telegram (or with a minimal probe).

**Cambrian reasoning.**
- SOTA capability: report bot permissions and webhook status.

**Output experience.**
- Human: mode and project route count.
- JSON: `brandi-telegram-status-v1`.

**Acceptance.**
- Missing config file returns exit 2 with the resolved default path.

## 5. Cross-cutting workstreams

### 5.1 Output layer

- Complete migration of all 38 commands to semantic events via `OutputSink`.
- Add JSON/JSONL schemas for every command (currently inconsistent).
- Add `--summary-only` for large outputs.
- Implement Barbara adapter behind a versioned contract; keep 3form fallback.

### 5.2 Brief/guideline reasoning integration

- Every command that loads brief/guidelines must emit a `GuidelineContext` event containing schema version, file paths, and validation status.
- Add `--brief-version` and `--guidelines-version` compatibility checks.
- Cache parsed brief/guidelines across portfolio runs.

### 5.3 Security and audit

- Every mutation command must produce an immutable receipt appended to `.brandi/state/security-audit.jsonl`.
- Pairing codes, tokens, and draft content must never appear in logs or events.
- Role checks must be centralized in `automation::authorize_local` and `automation::authorize_telegram`.

### 5.4 State durability

- Replace JSONL queues with SQLite transactions (REL-01).
- Add idempotency keys to promotion and ADB deliveries.
- Add state-machine property tests for approval → staging → publication.

### 5.5 Scale and performance

- Share one scan result between `scan`, `lint`, `propose`, and `reshoot`.
- Add incremental content hashing.
- Bound memory for 100,000-surface scans.

## 6. Sequencing

1. **Phase A — Output kernel**: finish `OutputSink` migration and schema inventory for all 38 commands.
2. **Phase B — Validation and scan**: harden `brief validate`, `guidelines validate`, `scan`, `lint`, `evaluate` with corpus evidence.
3. **Phase C — Assets and social**: elevate `assets *` and `social *` to SOTA with richer analysis and exports.
4. **Phase D — Mutation commands**: hardened confirmation, preview, and audit for `direct`, `reshoot`, ADB, promotion, tape render.
5. **Phase E — Daemon and Telegram**: background reliability and observability.
6. **Phase F — Integration**: SARIF, GitHub Checks, Kaptaind webhooks, service files.

## 7. Acceptance criteria

- Every leaf command has a checked-in specification covering ontology, coverage, output modes, safety class, and acceptance tests.
- Padagonia graph includes all 38 leaves, all parameters, all output dimensions, and no orphan nodes.
- Every command supports `--format human|json`; streaming commands support `--format jsonl`.
- Every mutation command has preview, confirmation, receipt, and audit coverage.
- All 28,728 finite safety-core cases are generated and constrained with evidenced exceptions.
- Self-lint score remains at least 90 with zero error-severity findings.

## 8. References

- `CLI_OUTPUT_EXPERIENCE_PLAN.md` — output policy, renderer architecture, phases.
- `ENTERPRISE_ROADMAP.md` — 26-week GA programme, workstreams, exit gates.
- `docs/PADAGONIA_OUTPUT_COVERAGE_AUDIT.md` — current ontology audit results.
- `docs/PHASE1_COMPLETION.md` — completed Phase 1 security boundaries.
- `src/cli.rs` — live command definitions.
- `src/commands.rs` — current command implementations.
- `src/output/mod.rs` — output policy and event model.
