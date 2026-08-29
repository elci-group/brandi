# Brandi enterprise CLI output experience plan

Status: implemented (Brandi-side), with guarded Barbara capability fallback  
Scope: every Brandi command, nested command, generated help/error path, and output-affecting flag  
Primary renderer: 3form  
Optional high-fidelity renderer: Barbara

## Implementation status — 2026-08-28

Brandi now has one global output policy covering all 38 executable leaves and generated Clap help/error paths. Human output is centralized, machine output supports JSON and JSONL without decoration, 3form supplies semantic styling and progress frames, animation is terminal- and accessibility-aware, terminal width and Unicode are policy-controlled, and failures expose stable codes with remediation text. The complete global flag surface is `--format`, `--color`, `--animation`, `--fidelity`, `--unicode`, `--width`, `--quiet`, and `--verbose`.

Barbara activation is capability-gated and bounded. The installed Barbara interface has no versioned live-event ingestion protocol, so rich fidelities deterministically fall back to 3form; `--verbose` explains the fallback. This preserves the planned compatibility boundary without coupling command execution to an undocumented interface.

Padagonia coverage is checked in at `tests/output_ontology.rs`: it derives the live Clap tree, verifies 38 leaves and their modeled parameters, round-trips the ontology graph, enumerates the 28,728-case finite safety core, and proves pairwise plus high-risk three-way presentation coverage. Runtime policy resolution is separately exhausted across 5,184 combinations in `src/output/mod.rs`.

## Executive decision

Brandi should use **3form as the in-process, deterministic rendering foundation** and **Barbara as an optional high-fidelity visualization backend**. Brandi must remain fully usable, accessible, and automation-safe when Barbara is absent, incompatible, slow, or disabled.

This is not a project to add colour to individual `println!` calls. The durable design is:

1. Command/domain code emits typed output events and typed final results.
2. One output policy resolves format, stream, terminal capability, accessibility, and operator preference.
3. Renderers consume the same semantic events:
   - plain text for pipes, logs, screen readers, and fallback;
   - 3form for styled human output and restrained terminal motion;
   - JSON/JSONL for machines;
   - Barbara for explicitly selected rich visualization.
4. Tests prove that all renderers preserve the same facts, exit semantics, and safety information.

Animation is appropriate only while useful work is in progress or state is changing. Help, validation findings, confirmation tokens, final results, redirected output, and machine formats should be static.

## Current-state audit

The current Clap model defines 38 executable leaf commands and 87 argument declarations. The leaf commands are:

- `direct`, `reshoot`, `init`, `scan`, `propose`, `check`, `lint`, `evaluate`
- `brief validate`, `guidelines validate`
- `assets check`, `assets list`, `assets audit`
- `social graph`, `social plan`
- `social adb status`, `social adb stage`, `social adb publish`
- `social adb wifi pair`, `social adb wifi connect`, `social adb wifi disconnect`, `social adb wifi bridge`
- `daemon run`, `daemon start`, `daemon stop`, `daemon status`
- `tape generate`, `tape validate`, `tape render`
- `promotion plan`, `promotion stats`, `promotion sync`, `promotion milestones`, `promotion queue`, `promotion approve`, `promotion reject`
- `telegram run`, `telegram status`

Generated output also exists at the root and every command-group level through `--help`, at the root through `--version`, and on all Clap parse/validation failures.

Important findings:

- Human output is assembled through direct `print!`/`println!`/`eprintln!` calls in `commands.rs`, `daemon.rs`, `automation.rs`, and other modules. This prevents one consistent policy from governing streams, styling, animation, and testing.
- Sixteen command branches already expose JSON. JSON coverage is inconsistent across the other commands.
- 3form is already linked and used by the lint report, but not by the rest of the command surface.
- 3form currently provides ANSI primitives, a chainable style API, tables, environment colour detection, and spinner frame selection. It deliberately does not provide a timing/runtime layer. Its caller must handle per-stream TTY detection, cursor lifecycle, interruption, and reduced motion.
- 3form tables currently need stronger terminal-width behavior before they can safely render arbitrary paths/content in narrow terminals.
- Barbara currently exposes project analysis and fidelity-based rendering (`static`, `storyboard`, `sketch`, `fill`, `print`, `show`, `movie`, and `console`). The installed interface analyzes a project snapshot; it does not expose a documented, versioned live-event ingestion contract suitable for embedding in every Brandi command.
- Barbara's generated configuration path is named `.baraba`; that spelling and schema must be treated as an external compatibility contract until Barbara defines a migration.
- Baseline verification currently fails to compile tests because `src/report.rs` calls `form3::term::set_override(false)`, but the linked 3form API does not provide that function. Resolve this before or in Phase 1; do not conceal it inside the visual refactor.

## Padagonia ontology and parameter-coverage model

Padagonia is the authoritative coverage graph for this initiative. It stores the relationships among the live Clap command model, command parameters, output-policy dimensions, semantic events, renderers, requirements, equivalence partitions, constraints, and generated tests. It does not replace a covering-array generator; it makes the inputs, exclusions, provenance, and resulting coverage queryable and auditable.

The graph uses these node classes:

- `OutputExperience`, `CommandFamily`, `LeafCommand`, `CommandParameter`
- `ParameterDimension`, `EquivalencePartition`, `Constraint`
- `OutputEvent`, `Renderer`, `CoverageRequirement`, `TestCase`
- `CombinationStrategy`, `Evidence`, `Exception`

Core relations are `contains`, `accepts`, `governed_by`, `partitioned_as`, `constrained_by`, `may_emit`, `must_emit`, `rendered_by`, `must_cover`, `covered_by`, `evidenced_by`, and `excepted_by`.

The command and parameter nodes must be generated from `Cli::command()` through Clap's `CommandFactory`; they must not be maintained as a second handwritten command list. Proposed global presentation dimensions are merged into that extracted graph. Every graph record carries provenance identifying the source file/schema and generator version.

### Parameter domains and partitions

Literal exhaustive enumeration is undefined for paths, strings, IDs, content, terminal widths, and environment values. “Full parameter coverage” therefore means full coverage of all **valid constrained equivalence-class combinations**, plus explicit invalid and boundary partitions. Each parameter is assigned one domain:

- boolean: false/default and true;
- enum: every declared value plus invalid value and case/alias behavior;
- numeric: missing/default, minimum, just above minimum, nominal, maximum, just above maximum, malformed, and overflow;
- path/file/list: default, valid relative, valid absolute, missing, unreadable, directory-vs-file mismatch, symlink, escape attempt, empty list, one item, many items, and bounded overflow;
- free string/ID/endpoint: default where applicable, empty, nominal, wide Unicode, multiline, control sequence, maximum length, oversized, and malformed grammar;
- secret source: absent, environment-provided, explicit flag, both sources with defined precedence, invalid, and redaction verification;
- confirmation: missing, exact match, mismatch, stale/expired, replay, and correct value bound to the wrong target/revision.

Constraints are first-class graph nodes, not filtering code hidden inside a test generator. Examples include “machine format disables decoration,” “`--portfolio` changes omitted-path semantics,” “publish requires an exact confirmation,” and “pairing code may come from flag or environment but must never be rendered.” Every excluded tuple needs an `Exception` node containing a reason and evidence.

### Combination tiers

Use four deterministic coverage tiers:

1. **Exhaustive finite safety core, every leaf:** `format × stdout_is_terminal × stderr_is_terminal × color × animation × outcome`. With the planned partitions this is `3 × 2 × 2 × 3 × 3 × 7 = 756` cases per leaf, or **28,728 cases across 38 leaves**, before constraint pruning.
2. **Exhaustive command-local safety tuples:** all combinations involving mutation/external-effect flags, confirmation, destination/target, secret source, format, and outcome for the commands that own them.
3. **Constrained 3-way coverage:** fidelity, Unicode, width, verbosity, cardinality, content class, renderer availability/failure, CI/terminal environment, and every command-specific non-safety parameter.
4. **Constrained pairwise coverage:** all remaining cross-command presentation parameters, followed by property-based boundary and hostile-input generation.

This policy gives exhaustive coverage where an interaction can corrupt a machine stream, expose a secret, damage terminal state, or misrepresent a mutation. It avoids pretending that an infinite Cartesian product can be executed. The generator must publish its seed, constraint set, uncovered tuples, and minimized case list as Padagonia evidence.

### Graph invariants and CI queries

The CI audit fails if any of these queries returns a result:

- a Clap leaf has no unique `LeafCommand` node;
- a command parameter has no domain or equivalence partitions;
- a leaf lacks any global output dimension or mandatory coverage requirement;
- a finite command lacks human, plain, and machine renderers;
- a streaming command lacks plain newline and JSONL contracts;
- a mutation/external command lacks preview, confirmation where required, state-change, receipt, exit, and redaction coverage;
- an animatable phase lacks cancellation, broken-pipe, terminal-cleanup, redirected-output, and reduced-motion cases;
- an expected tuple has no `covered_by` test edge and no reasoned `excepted_by` edge;
- a test points to a removed command, parameter, partition, event, or requirement;
- competing ontology facts lack provenance or fall below the accepted confidence threshold.

Generate `.pad` and JSON projection files in CI as retained audit artifacts; do not commit the binary graph. Snapshot/restore validation and a root BFS must succeed before the coverage report is accepted.

## Non-negotiable output contracts

1. **Machine output is inviolate.** `--format json` emits exactly one valid JSON document on stdout, with no ANSI, progress, warning, banner, or Barbara bytes. If JSONL is added, each stdout line is one schema-valid event. Diagnostics go to stderr or into the declared document schema.
2. **Pipes are stable.** Redirected stdout defaults to plain, static, non-Unicode-safe text unless the user explicitly forces another mode.
3. **Meaning never depends on decoration.** Every colour, icon, animation, or spatial relationship has a textual label.
4. **Final state survives interruption.** Spinners and transient regions always clear on success, failure, Ctrl-C, panic-hook execution, and broken pipe. The terminal cursor is always restored.
5. **Safety information is static and complete.** Preview, actor, project, target, destination, revision/content hash, consequence, and exact confirmation token are never hidden in animation or truncated by default.
6. **Exit codes do not change with presentation.** Renderer selection cannot affect command behavior or exit status.
7. **No untrusted terminal control.** User/project content is sanitized before terminal rendering. Only renderer-generated control sequences may reach a terminal.
8. **No required external renderer.** Missing or failed Barbara degrades to the selected 3form/plain fidelity without failing the underlying command.
9. **Determinism is available.** Tests, CI, recordings, and support bundles can force fixed width, static motion, no colour, no timestamps, and stable ordering.
10. **Secrets remain data-classified.** Pairing codes, tokens, environment values, and sensitive content are never placed in renderer events or diagnostic context.

## Target architecture

### 1. Separate command execution from presentation

Refactor command functions so they return typed results and publish semantic events through a narrow `OutputSink` rather than printing directly.

Suggested modules:

```text
src/output/
  mod.rs          OutputContext, OutputSink, Renderer, stream routing
  event.rs        versioned OutputEvent and semantic field types
  policy.rs       precedence, TTY/capability, accessibility decisions
  theme.rs        semantic design tokens and Brandi palette mapping
  plain.rs        zero-control-sequence fallback
  form3.rs        styled tables, diagnostics, progress, transient regions
  json.rs         JSON and JSONL contracts
  barbara.rs      optional adapter, handshake, bounded fallback
  help.rs         root/group/command help and parse diagnostics
  sanitize.rs     terminal-control and untrusted-content handling
```

The renderer interface should own both output streams and lifecycle:

```rust
trait OutputSink {
    fn emit(&mut self, event: OutputEvent) -> io::Result<()>;
    fn finish(&mut self, outcome: CommandOutcome) -> io::Result<()>;
}
```

Command code may report domain progress but must not select colours, glyphs, animation frames, or terminal streams.

### 2. Use a versioned semantic event model

Start with `brandi-output-event-v1`. Events should include only fields appropriate to their kind:

- `CommandStarted { command, correlation_id, project }`
- `PhaseStarted { id, label, total }`
- `Progressed { phase, current, total, item }`
- `Item { kind, primary, fields }`
- `Diagnostic { severity, code, message, path, line, action, docs }`
- `Preview { actor, project, destination, content_hash, consequence, confirmation }`
- `StateChanged { subject, from, to }`
- `CommandFinished { status, summary, counts, elapsed }`

Events need monotonic sequence numbers. Wall-clock timestamps should be optional and excluded from deterministic render mode. Free-form messages are allowed only as display text; stable codes and structured fields remain authoritative.

### 3. Resolve one immutable output policy at process start

Add global Clap arguments with `global = true` so they work before or after nested commands:

- `--format human|json|jsonl` (retain current behavior; add JSONL only with a documented schema)
- `--color auto|always|never`
- `--animation auto|always|never`
- `--fidelity auto|static|storyboard|sketch|fill|print|show|movie`
- `--unicode auto|always|never`
- `--width <columns>` for deterministic tests/support output
- `--quiet` for final outcome only
- `--verbose` for phase-level diagnostics

Do not expose Barbara's `console` fidelity initially. It is not an appropriate implicit terminal mode and needs a separate security, portability, and resource review.

Precedence should be: explicit CLI flag, Brandi environment variable, project config, user config, capability detection, safe default. Proposed environment variables are `BRANDI_FORMAT`, `BRANDI_COLOR`, `BRANDI_ANIMATION`, `BRANDI_FIDELITY`, and `BRANDI_UNICODE`; also honor `NO_COLOR`, `TERM=dumb`, `CI`, and per-stream `IsTerminal`.

Configuration belongs in `.brandi/output.yaml` with a version field. Barbara-specific project metaphor/config remains in its own external file and is read only when Barbara mode is selected.

### 4. Give stdout and stderr explicit ownership

- stdout: requested result/document only.
- stderr: transient progress, warnings that are not part of a machine document, and human error diagnostics.
- JSON: no human progress on stdout; stderr is plain and static by default.
- JSONL: event stream on stdout only when explicitly selected; process diagnostics use stderr.
- foreground daemon/Telegram logs: durable newline records, never cursor rewriting.

Every renderer should accept injected writers. This makes output testable without process-global capture and prevents library/domain code from writing around the policy.

## 3form workstream

Use 3form for the full baseline experience, but close these gaps first:

1. Add an explicit, test-safe capability override API or remove Brandi's dependency on the missing `set_override` call by injecting `OutputCapabilities`.
2. Add `TrueColor` to colour capability negotiation while preserving 16-colour fallback.
3. Add Unicode display-width and wrapping/truncation support that understands ANSI and wide graphemes.
4. Make table colour honor an injected colour policy rather than always emitting SGR for coloured cells.
5. Add a small transient-region runtime in Brandi (or upstream in 3form): delayed start, tick scheduling, redraw, final-line replacement, cursor hide/show guard, and panic/drop cleanup.
6. Add progress bar and bounded list/table primitives; do not turn 3form into a domain framework.
7. Provide semantic style adapters for headers, options, values, paths, identifiers, counts, statuses, warnings, errors, and confirmation tokens.

Brandi's theme should use semantic tokens rather than raw colours:

- `brand`, `heading`, `key`, `value`, `path`, `code`, `muted`
- `success`, `warning`, `danger`, `info`, `pending`, `changed`
- `focus`, `confirmation`, `destructive`, `external`

Each token maps to true colour, ANSI-16, monochrome attributes, and plain text. Status labels remain present in every mapping.

## Barbara workstream

Barbara should consume the same semantic events only after it exposes a supported integration contract. The preferred order is:

1. **Library adapter:** a versioned Barbara Rust crate that accepts Brandi events through a trait and renders to an injected terminal.
2. **Sidecar protocol:** a long-lived `barbara render-events` process using versioned JSONL over stdin/stdout with a startup handshake.
3. **Snapshot export:** Brandi writes a bounded event snapshot and invokes Barbara once after execution. This is acceptable for replay/demos, not live command progress.

Do not invoke `barbara render` once per event and do not parse Barbara's decorated human output.

The adapter requires:

- protocol and schema version negotiation;
- a 250 ms startup budget and bounded event queue;
- backpressure that drops/coalesces progress events but never preview, warning, failure, or final events;
- subprocess timeout, clean termination, and no inherited secrets;
- sanitized environment and explicit project path;
- automatic fallback to 3form at the nearest lower fidelity;
- a one-line debug diagnostic only under `--verbose` when fallback occurs;
- deterministic storyboard mode for tests and VHS recordings.

Recommended fidelity mapping:

| Requested fidelity | Backend | Motion policy |
|---|---|---|
| `static` | plain/3form | none |
| `storyboard` | 3form | phase transitions only; deterministic |
| `sketch` | 3form | restrained spinner/progress |
| `fill` | 3form | sketch plus semantic colour |
| `print` | Barbara, then 3form fallback | branded metaphor, low motion |
| `show` | Barbara, then `fill` fallback | rich terminal layers |
| `movie` | Barbara, explicit only | immersive/replay use; never auto-selected |

`auto` should select at most `fill` for normal interactive use. `print`, `show`, and `movie` require explicit selection or a trusted project/user configuration. This avoids surprise CPU use, motion, and terminal takeover.

## Appropriate motion by command family

| Command family | During work | Final presentation |
|---|---|---|
| root/group/leaf `--help`, `--version`, parse errors | none | styled static sections and actionable diagnostics |
| `brief/guidelines validate`, `tape validate` | delayed spinner only if validation exceeds 200 ms | static pass/warn/fail list |
| `scan`, `lint`, `evaluate`, `assets audit` | delayed spinner; measurable file/case progress; coalesce to <=10 redraws/s | summary, findings table/list, completeness and exit reason |
| `check`, `assets check/list`, `social graph/plan`, status commands | normally none; delayed spinner for external probes | compact static result |
| `direct`, `reshoot`, `tape generate`, `promotion milestones/sync` | named phases and per-project/item progress | changed/skipped/failed summary |
| `tape render`, ADB stage/publish, promotion approve/reject | static preview first; motion only after valid confirmation | immutable action receipt with target/hash/status |
| ADB Wi-Fi pair/connect/disconnect/bridge | delayed connection spinner with timeout countdown | endpoint/action/result; pairing secret never echoed |
| `daemon run`, `telegram run` | newline lifecycle/log events; optional low-rate heartbeat | shutdown reason and counters |
| `daemon start/stop` | short delayed spinner | PID/identity/status receipt |
| queue and potentially large listings | no animation; paginate/filter/truncate by policy | totals plus deterministic rows |

Motion starts only after 200 ms to prevent flicker, redraws at no more than 10 Hz, and stops immediately when output is redirected or reduced motion is active. Unknown-duration operations use a spinner; known totals use progress. A completed animation resolves into one durable textual line.

## Help, flags, and parse diagnostics

Clap remains the source of truth for command/argument metadata. Build a help renderer from `clap::Command` metadata so root, group, and leaf help share:

- command synopsis and purpose;
- arguments, global options, defaults, allowed values, environment/config equivalents;
- mutation/external-effect badges written as text;
- examples for confirmation and machine-output use;
- stable ordering and width-aware wrapping.

If Clap's native styled error path remains, configure its `Styles` from the same Brandi theme adapter. Prefer intercepting `try_parse` errors in `main` so usage, invalid values, missing arguments, and unknown subcommands receive stable Brandi diagnostic codes and remediation without losing Clap's suggestions.

`--help` and `--version` are always static. `--color`, `--animation`, and `--fidelity` affect only human presentation. They are rejected or ignored with a documented warning when combined with machine formats; choosing one consistent behavior is required before release. Recommended behavior: accept the flags but force machine-safe output and emit nothing extra unless `--verbose` is active on stderr.

## Command-family presentation specifications

Each leaf command needs a checked-in specification recording:

- mutation/external/read-only classification;
- phases and measurable totals;
- semantic events and final result schema;
- stdout/stderr ownership;
- human, plain, JSON, and JSONL availability;
- animation eligibility and fallback;
- exit-code conditions;
- sensitive fields and redaction rules;
- narrow-width and empty-state behavior.

Normalize missing machine modes rather than styling only existing human strings. All finite commands should support `--format human|json`; add JSONL where streaming is valuable. Foreground supervisors should use JSONL rather than one unbounded JSON document.

## Delivery phases and gates

### Phase 0 — contracts and inventory

- Generate a command manifest from Clap and fail CI when a leaf command or option lacks an output specification.
- Build the Padagonia ontology from that manifest, materialize parameter partitions/constraints, and fail on orphan or uncovered nodes.
- Generate the exhaustive safety core and constrained covering arrays; publish every excluded tuple with a reason.
- Snapshot current human/JSON output and exit codes as migration fixtures.
- Define event, result, error, and configuration schemas.
- Decide whether Barbara will provide a library or sidecar contract; do not begin rich integration without it.
- Fix or intentionally replace the current 3form test override mismatch.

Gate: 100% of the 38 leaf commands, all help levels, version, and parse-error families are present in the manifest; Padagonia reports no orphan, unjustified exclusion, or uncovered required tuple.

### Phase 1 — output kernel and plain renderer

- Add `OutputContext`, event/result types, injected writers, sanitization, and stream routing.
- Convert direct prints at module boundaries into events/results.
- Implement static plain rendering and preserve existing JSON schemas where compatibility matters.
- Add global policy flags without enabling animation.

Gate: no production `print!`/`println!`/`eprint!`/`eprintln!` outside the output layer and explicitly approved daemon log adapter.

### Phase 2 — 3form styling

- Land the required 3form capability, width, table, and testability improvements.
- Implement the semantic theme and styled help/errors.
- Migrate reports, validation, status, queues, previews, and receipts family by family.

Gate: decorated and plain renderers are semantically equivalent after stripping decoration; accessibility checks pass.

### Phase 3 — restrained animation

- Add transient-region lifecycle, delayed spinner, progress, cancellation, and cleanup guards.
- Instrument long-running domain loops with real totals where available.
- Enable `auto` only for interactive TTYs and only for approved command phases.

Gate: PTY tests prove no cursor/escape leakage after success, failure, Ctrl-C, panic, broken pipe, or timeout.

### Phase 4 — machine completeness

- Add JSON to every finite leaf command and JSONL to streaming commands.
- Version schemas and publish compatibility policy.
- Add `--quiet`, `--verbose`, width, and deterministic support modes.

Gate: every machine-mode stdout parses; no ANSI or human warning is present; exit-code matrix is unchanged.

### Phase 5 — Barbara integration

- Implement the agreed versioned adapter and fallback circuit breaker.
- Ship `print` first, then `show`; keep `movie` experimental and explicit.
- Add event replay fixtures for support and VHS demonstrations.

Gate: Barbara absent, incompatible, crashing, stalled, or backpressured never breaks a Brandi command or corrupts its terminal.

### Phase 6 — rollout and governance

- Release behind `BRANDI_OUTPUT_V2=1`, then opt-in config, then default human renderer.
- Collect only privacy-safe performance/fallback counters when telemetry is explicitly enabled.
- Publish migration, accessibility, schema, and troubleshooting documentation.
- Remove legacy renderers only after two compatible release cycles.

Gate: support runbook, rollback switch, compatibility fixtures, and operator acceptance are complete.

## Verification matrix

The test program must cover:

- golden output for all 38 leaf commands in plain human and decorated human modes;
- JSON fixtures for every finite leaf command and JSONL schema validation for streams;
- root, group, leaf help; version; missing argument; invalid value; unknown flag; unknown command;
- TTY vs pipe vs file redirection for stdout and stderr independently;
- `NO_COLOR`, `TERM=dumb`, CI, forced colour, reduced motion, forced width, ASCII-only mode;
- 40/80/120/200-column terminals, long paths, wide Unicode, combining marks, multiline and hostile control-sequence input;
- empty, one-item, many-item, partial, warning, failure, timeout, cancellation, and broken-pipe states;
- renderer semantic-equivalence property tests;
- no-secret assertions for pairing codes, tokens, configuration values, and event snapshots;
- Barbara missing/version mismatch/crash/hang/slow-consumer fallback;
- animation frame-rate, CPU, allocation, and terminal-cleanup benchmarks;
- backwards-compatibility snapshots for existing JSON schemas and exit codes.
- Padagonia save/load/snapshot validation, ontology traversal, provenance, orphan detection, and coverage-edge completeness.
- deterministic regeneration of the 28,728-case finite safety core and all constrained covering arrays.

Run unit tests, Rust integration tests with a pseudo-terminal, shell-level contract tests, and representative screen-reader/manual accessibility review. Keep deterministic VHS recordings as visual regression evidence, not as the sole assertion mechanism.

Performance budgets for output itself:

- static renderer overhead below 2% of command wall time and below 5 MiB peak additional RSS;
- progress redraw work below 1 ms per frame at 10 Hz;
- first durable output within 100 ms when data is available;
- Barbara startup/fallback decision within 250 ms;
- bounded event queue with constant-memory behavior for 100,000-item scans.

## Security and operational review

- Treat terminal escape injection as an output-layer security issue and fuzz sanitization.
- Never pass the full inherited environment to a Barbara sidecar; allowlist only terminal and locale settings.
- Never serialize secrets into replay files, logs, diagnostics, or correlation metadata.
- Do not let output failure repeat a mutation. Rendering and domain execution have separate idempotency boundaries.
- If stdout closes, stop expensive presentation work and return the conventional broken-pipe behavior without a panic dump.
- Correlation IDs appear in errors and receipts, but are random/non-sensitive and excluded from deterministic snapshots.
- Barbara fidelity config is trusted only from the selected project root after path and symlink confinement checks.

## Definition of done

The initiative is complete when:

1. Every leaf command, help/version path, parse error, and output-affecting flag is represented in the generated manifest.
2. Padagonia can traverse from the output-experience root to every command, parameter, partition, constraint, requirement, renderer, event, and generated test; every required combination is covered or has an approved, evidenced exception.
3. Every finite command has a styled human renderer, static plain renderer, and schema-valid machine renderer.
4. Every long operation has either measurable progress or an intentionally documented static/delayed-spinner policy.
5. No status or safety fact relies on colour, glyph, layout, or motion alone.
6. Redirected, CI, reduced-motion, and `NO_COLOR` behavior is deterministic.
7. Barbara failure is transparent to command correctness and terminal integrity.
8. All output, PTY, accessibility, security, compatibility, and performance gates pass.
9. README, command reference, schema documentation, configuration reference, support runbook, and the enterprise roadmap match shipped behavior.
