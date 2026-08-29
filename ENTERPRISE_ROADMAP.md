# Brandi enterprise-readiness roadmap

## Executive objective

Move Brandi from a strong prototype (50.4% in the August 2026 fifty-factor review) to an enterprise-grade, generally available product in 26 weeks.

The target is not merely a higher review score. General availability requires Brandi to be safe around generated commands and publication, precise enough to trust in large repositories, observable under failure, transactionally reliable, supportable across declared platforms, and reproducibly shipped from an independent repository.

## Enterprise exit criteria

Brandi may be declared generally available only when all of the following are true:

- No open critical or high-severity security findings.
- No shell command reaches execution from generated content without a typed allowlist and an isolated execution boundary.
- Every external action is deny-by-default, authenticated, authorized, attributable, previewable, and auditable.
- Scanner precision is at least 95% and recall at least 90% on a versioned representative corpus.
- Brandi's self-lint scores at least 90 without hiding genuine product copy or using blanket exclusions.
- Critical workflow mutation tests demonstrate no lost, duplicated, or partially committed state.
- Traci reports zero criticals and zero errors in production code; approved warnings have owners and expiry dates.
- Overall Rust line coverage is at least 80%; security and state-transition modules are at least 90%.
- CLI dispatch, daemon lifecycle, approval, publication, tape execution, and recovery paths have integration tests.
- CI enforces formatting, linting, tests, coverage, dependency audit, SBOM generation, licensing, and reproducible release builds.
- Supported operating systems and architectures pass the same acceptance suite.
- Signed release artifacts, checksums, provenance, rollback instructions, and an operator runbook exist.
- Service-level objectives, telemetry, incident ownership, and support escalation are documented and exercised.
- The fifty-factor review reaches at least 220/250, with no factor scored 0 or 1.

## Programme principles

1. Safety precedes feature expansion. Freeze new publication, device-control, and generated-execution features until Phase 1 exits.
2. Deny by default. Missing identity, policy, allowlist, confirmation, or audit storage must stop an external action.
3. Separate planning from execution. Plans are inert data until a separately authorized executor validates them.
4. Make failures reconstructable. Every external call, state transition, and background task carries a correlation ID and structured outcome.
5. Measure precision, not just test counts. Scanner changes require corpus-level precision and recall evidence.
6. Prefer one source of truth. Rust owns domain schemas and rule metadata; other clients consume generated contracts.
7. Treat local state as a database. Durable queues require transactions, idempotency, locking, and migration discipline.
8. Ship narrowly. Unsupported platforms and integrations are stated explicitly rather than appearing to work accidentally.

## Delivery model and staffing

Recommended core team for the 26-week plan:

- One programme/product lead: scope, customer requirements, acceptance, and launch.
- Two Rust platform engineers: scanner, policy engine, daemon, state, performance, and packaging.
- One security/platform engineer: sandboxing, identity, authorization, threat modelling, and supply chain.
- One Go/product engineer: TUI, generated contracts, accessibility, and operator workflows.
- One quality/SRE engineer: integration tests, corpus evaluation, chaos tests, observability, CI, and release qualification.
- Shared technical writer/design support during Phases 4 and 5.

A three-person team should plan for 9-12 months. The 26-week target assumes five full-time engineers plus product leadership and prompt access to security review.

## Workstreams

### GOV — governance and repository foundation

#### GOV-01: establish an independent repository

Deliverables:

- Move Brandi into a dedicated repository while preserving authorship and relevant history where available.
- Protect the default branch; require reviewed pull requests and passing status checks.
- Add `CODEOWNERS`, contribution guidance, security policy, support policy, code of conduct, changelog, and an approved license.
- Define release, hotfix, deprecation, and vulnerability-response policies.
- Remove checked-in or stray binaries; make generated artifacts unambiguously ignored.

Acceptance:

- Every source and policy file is tracked.
- No source release is built from an uncommitted or unreviewed tree.
- Repository settings are verified by an automated governance check.

#### GOV-02: define product boundaries and ownership

Deliverables:

- Publish a supported-feature matrix for linting, assets, daemon, tape, promotion, Telegram, ADB, and TUI.
- Mark experimental capabilities and place them behind explicit feature flags.
- Expand target-user research beyond one platform-engineer persona.
- Assign an owner and lifecycle state to every command and integration.

Acceptance:

- No GA command lacks an owner, support tier, threat model, and documented data boundary.
- Experimental integrations cannot be enabled accidentally in default builds.

### SEC — execution, identity, and supply-chain security

#### SEC-01: replace the VHS denylist with a typed execution policy

Deliverables:

- Represent tape scenes as typed executable plus argument arrays; do not accept arbitrary shell text for execution.
- Maintain a minimal command allowlist with per-command argument validators and working-directory constraints.
- Run generated demonstrations inside an isolated, resource-limited sandbox with no network, no host credentials, a read-only source mount, and a disposable output directory.
- Require an execution preview containing the exact normalized command plan and policy decision.
- Record the plan hash, approver, sandbox policy, executable digest, start/end times, and result.
- Add adversarial tests for interpreter, path, environment, quoting, Unicode, symlink, nested-shell, and executable-substitution bypasses.

Acceptance:

- Security review cannot produce a command-execution bypass.
- Fuzzing and the adversarial suite pass in CI.
- `tape render` fails closed when the sandbox or policy engine is unavailable.

#### SEC-02: implement authenticated, authorized external actions

Deliverables:

- Reject empty Telegram allowlists unless an explicit, prominently warned public mode is enabled.
- Separate viewer, planner, approver, publisher, device-operator, and administrator roles.
- Bind identities to credentials; remove caller-supplied actor strings as authority.
- Require exact revision hash or ID confirmation for approval and publication.
- Add a second confirmation for TUI publication-affecting actions and display destination, content, and actor.
- Use expiring approvals and prevent approval from one channel authorizing a different destination.
- Store an immutable audit event for every allowed and denied action.

Acceptance:

- Authorization matrix tests cover every command and role.
- Anonymous, stale, cross-project, replayed, and cross-destination actions are denied.
- A single accidental keypress cannot approve or publish content.

#### SEC-03: resource and dependency controls

Deliverables:

- Add maximum file size, decoded image dimensions, pixel count, archive depth, response size, subprocess duration, and concurrency limits.
- Add connection, total-request, and idle timeouts with bounded exponential backoff and jitter.
- Pin and audit Rust and Go dependency graphs; remove or replace unmaintained transitive dependencies where feasible.
- Replace deprecated YAML handling or formally isolate and monitor it until migration.
- Generate an SBOM and license inventory for every release.

Acceptance:

- Decompression-bomb, oversized-repository, slow-network, hung-process, and response-flood tests remain within declared CPU, memory, and time budgets.
- Dependency and license policy gates pass with no unapproved exceptions.

### DET — scanner, rules, and scoring quality

#### DET-01: build an AST-aware surface extraction layer

Deliverables:

- Define a language-adapter interface and implement AST-backed Rust, Go, JavaScript/TypeScript, and Python extractors.
- Recognize UI, CLI, logging, localization, template, and documentation contexts explicitly.
- Exclude tests, fixtures, generated code, dependency trees, private memory, and machine-only strings by policy rather than ad hoc per-file exceptions.
- Preserve source spans and explain why each candidate was classified as outward-facing.
- Report unreadable paths and parse failures as scan diagnostics; never silently convert them into a clean result.
- Version the extraction schema and keep regex fallback opt-in for unsupported languages.

Acceptance:

- Precision >=95% and recall >=90% on the evaluation corpus.
- Every skipped file is represented in machine output with a reason.
- Self-scan no longer treats test fixtures and internal format fragments as product copy.

#### DET-02: establish a representative evaluation corpus

Deliverables:

- Curate small, medium, monorepo, multilingual, generated-code, documentation-heavy, and design-system fixtures.
- Label true surfaces, non-surfaces, expected findings, and intended severities.
- Add mutation cases for casing, punctuation, Markdown fences, templates, escaped strings, colors, localization, and broken assets.
- Track precision, recall, false-positive clusters, false-negative clusters, and rule stability by release.

Acceptance:

- Corpus metrics are produced on every pull request affecting extraction or rules.
- Regressions beyond agreed tolerances block merging.

#### DET-03: redesign scoring and policy semantics

Deliverables:

- Replace raw global subtraction with a documented model that accounts for severity, rule, surface type, confidence, and exposure while remaining explainable.
- Report absolute findings and normalized density separately.
- Distinguish incomplete scans from clean scans and prevent incomplete evidence from scoring 100.
- Version scoring models so historical daemon charts remain interpretable.
- Add baseline/delta adoption mode without suppressing newly introduced violations.
- Expand optional rules for accessibility language, inclusive terminology, reading level, localization readiness, and terminology ownership.

Acceptance:

- Scores remain meaningful from ten surfaces to enterprise monorepos.
- The same unchanged project produces stable scores under repeated runs.
- Historical reports identify their scoring-model version.

### REL — durable state, observability, and recovery

#### REL-01: replace JSONL mutation state with transactions

Deliverables:

- Use SQLite or an equivalent transactional local store for queues, outbox records, approvals, deliveries, offsets, and migrations.
- Add unique idempotency keys, optimistic concurrency/version columns, and explicit state-transition constraints.
- Make external delivery at-least-once with destination-side idempotency where supported; expose ambiguous outcomes for operator reconciliation.
- Protect concurrent daemon, TUI, CLI, Telegram, and ADB access with transactions rather than process convention.
- Retain exportable append-only audit records with integrity checks.

Acceptance:

- Parallel writer and crash-injection tests show no lost updates, invalid transitions, or corrupted records.
- Restart after each simulated crash point produces a deterministic recoverable state.
- Store migrations support rollback or a documented forward-recovery procedure.

#### REL-02: close the observability backlog

Deliverables:

- Add structured tracing with correlation IDs across CLI, daemon, subprocesses, network requests, queue transitions, and worker threads.
- Preserve source errors and classify retryable, permanent, policy, and operator errors.
- Replace discarded results with explicit handling, structured warnings, or narrowly justified exceptions.
- Add metrics for scan duration, skipped files, queue depth, delivery attempts, retries, policy denials, daemon health, and worker lag.
- Define log redaction tests so tokens, pairing codes, content drafts, and authorization headers cannot leak.

Acceptance:

- Traci reports zero criticals and zero errors.
- A support engineer can reconstruct every external delivery attempt from one correlation ID.
- Secret-scanning tests pass against logs and failure messages.

#### REL-03: operational resilience

Deliverables:

- Add supervised shutdown, cancellation, health/readiness endpoints, worker heartbeats, and bounded queues.
- Implement exponential backoff, circuit breakers, and retry budgets for external services.
- Distinguish daemon process identity from a generic command-line substring; persist a start token or process fingerprint.
- Add backup, restore, compaction, retention, and disaster-recovery procedures for local state.
- Exercise crash, disk-full, permission, corrupted-state, network-partition, stale-PID, and clock-shift scenarios.

Acceptance:

- Recovery exercises meet the defined recovery-time and recovery-point objectives.
- No retry loop can consume unbounded CPU, network, disk, or paid API quota.

### ARC — architecture, contracts, and dependencies

#### ARC-01: split high-entropy modules

Deliverables:

- Split `automation.rs` into metrics, planning, queue, outbox, Telegram transport, authorization, and supervisor modules.
- Split `rules.rs` into a catalog, rule interface, individual rule modules, and evaluation pipeline.
- Split `assets.rs` into discovery, decoding, classification, metrics, duplicate detection, and reporting.
- Keep public APIs narrow and document ownership and allowed dependencies between layers.
- Add architecture checks for cycles, dependency direction, entropy, and module-size budgets.

Acceptance:

- Fract health >=90%, no critical modules, and no module above the agreed entropy ceiling.
- Core domain logic has no direct process, network, terminal, or filesystem dependency.

#### ARC-02: generate cross-language contracts

Deliverables:

- Define versioned report, finding, rule-catalog, queue, daemon, and status schemas.
- Generate Go types and rule metadata from the authoritative schema.
- Add compatibility tests for old and new producers and consumers.
- Reject unsupported major schema versions with an actionable error.

Acceptance:

- No manually duplicated rule catalog or wire-format type remains in the TUI.
- Contract drift is caught during CI before either binary is released.

#### ARC-03: reduce dependency and binary cost

Deliverables:

- Review Amber candidates, beginning with `colored`, `chrono`, and `walkdir`.
- Restrict image codec features to formats Brandi actually supports.
- Introduce release size and compile-time budgets.
- Keep dependency removals evidence-led; do not replace mature security-sensitive libraries with weaker local code.

Acceptance:

- Every direct dependency has a documented purpose and owner.
- Release artifact size and clean build time meet published budgets without functionality regressions.

### QAT — tests, verification, and performance

#### QAT-01: integration and state-machine testing

Deliverables:

- Test CLI parsing and dispatch, exit codes, JSON contracts, daemon start/stop/recovery, TUI-to-CLI integration, and configuration validation.
- Model approval and delivery as a state machine and property-test all legal and illegal transitions.
- Add fake Telegram, Groq, GitHub, Padagonia, ADB, VHS, Git, and filesystem adapters.
- Add end-to-end tests that never contact real networks or devices.
- Add fault injection before and after every durable write and external call.

Acceptance:

- Overall Rust coverage >=80%; security, authorization, queue, outbox, and publication modules >=90%.
- Main and command-dispatch coverage is no longer zero.
- Go coverage includes keyboard safety, command construction, schema compatibility, and error states.

#### QAT-02: fuzzing and property testing

Deliverables:

- Fuzz brief/guidelines YAML, ignore patterns, source extraction, Markdown parsing, regex generation, tape plans, UI XML parsing, and JSONL migration input.
- Property-test path confinement, scoring bounds, idempotency, serialization round trips, and deterministic plans.
- Add sanitizers and undefined-behaviour checks where supported.

Acceptance:

- Scheduled fuzzing has no unresolved reproducible crash or policy bypass.
- Security-sensitive parsers meet an agreed minimum fuzzing duration before release.

#### QAT-03: scale and performance qualification

Deliverables:

- Benchmark cold and incremental scans, asset analysis, rule evaluation, daemon response, TUI refresh, and state operations.
- Avoid scanning the tree twice for one lint; share a single immutable scan result.
- Add incremental content hashing and cache invalidation keyed by policy and extractor versions.
- Stream or bound file reads and parallelize only with deterministic ordering and bounded workers.

Initial performance objectives, to be validated against customer repositories:

- 10,000-file repository: cold lint p95 under 10 seconds on the reference workstation.
- 100,000-file repository: cold lint p95 under 60 seconds and peak RSS under 1 GiB.
- Single-file incremental lint: p95 under 2 seconds.
- TUI refresh: no blocking frame over 100 ms; long operations are cancellable.

Acceptance:

- Performance budgets run in a controlled benchmark job and block material regression.

### UX — safe operator experience

#### UX-01: command and TUI safety

Deliverables:

- Add preview, confirmation, cancellation, timeout, and result states for every mutation.
- Display actor, project, destination, content hash, and consequence before approval or publication.
- Distinguish plan, approval, staging, publication, and delivery outcomes consistently.
- Add non-interactive confirmation flags suitable for CI without weakening authorization.
- Ensure JSON output never mixes machine data with warnings on stdout.

Acceptance:

- Destructive or external actions pass usability testing with no accidental completion.
- Every state-changing command supports deterministic automation and human-readable preview.

#### UX-02: accessibility and supportability

Deliverables:

- Ensure status is never conveyed by color or emoji alone.
- Support narrow terminals, reduced animation, no-color mode, keyboard discovery, and screen-reader-friendly text output.
- Add concise scan summaries, pagination/filtering, and a `--summary-only` option.
- Replace vague failures with an action, stable error code, correlation ID, and documentation link.

Acceptance:

- TUI and CLI pass the project's accessibility checklist and representative assistive-technology review.
- A 100,000-surface scan can be investigated without emitting or manually navigating 100,000 lines.

### PLT — platform and release engineering

#### PLT-01: portable process abstraction

Deliverables:

- Isolate process lifecycle, filesystem watching, signaling, and process identity behind platform adapters.
- Declare the supported matrix explicitly: Linux first, followed by macOS and Windows only after acceptance passes.
- Replace direct `/proc` and Unix process-group assumptions outside the Linux adapter.
- Define service-manager guidance for systemd, launchd, and Windows Service deployment as applicable.

Acceptance:

- Each supported target passes build, unit, integration, daemon, path, and packaging tests on native runners.
- Unsupported features fail with a clear platform message rather than partial behaviour.

#### PLT-02: reproducible CI and releases

Deliverables:

- Enforce formatting, Clippy, rustdoc, Go formatting/vet/tests, Deliver, Brandi self-lint, Traci, coverage, audit, license, and schema checks.
- Produce optimized, stripped CLI and TUI binaries for supported targets.
- Generate checksums, signatures, SBOMs, provenance, release notes, and upgrade/rollback instructions.
- Verify a clean checkout reproduces release artifacts within the chosen reproducibility boundary.
- Add artifact retention and build-cache limits; keep coverage output separate from normal target artifacts.

Acceptance:

- Release creation is automated, reviewed, repeatable, and cannot bypass required gates.
- No debug or ambiguously named binary ships.

### BRD — product coherence and documentation

#### BRD-01: make Brandi pass its own policy credibly

Deliverables:

- Fix true outward-facing self-lint findings.
- Remove false positives through extractor policy and corpus improvements, not blanket score suppression.
- Add genuine brand assets, usage examples, and accessibility review where product distribution needs them.
- Expand audience research and narrative mappings while keeping generated claims evidence-bound.
- Add rule and scoring documentation with examples of correct, incorrect, skipped, and uncertain classifications.

Acceptance:

- Self-lint >=90 with zero error-severity findings.
- Every exclusion has a specific reason and can be audited in scan output.

#### BRD-02: enterprise documentation set

Deliverables:

- Architecture, threat model, data-flow, configuration, deployment, operations, backup/restore, migration, incident, and troubleshooting guides.
- Administrator and least-privilege role guides for Telegram, ADB, promotion, and tape execution.
- API/schema compatibility policy and deprecation schedule.
- Quickstarts for evaluation, CI-only use, local daemon use, and controlled external integrations.

Acceptance:

- A new operator can deploy, upgrade, diagnose, back up, restore, and roll back without developer intervention.

## Phased schedule

### Phase 0 — control the baseline (weeks 0-2)

Scope: GOV-01, GOV-02, CI foundations from PLT-02, and threat-model discovery.

Exit gate:

- Independent tracked repository and protected branch.
- Formatting passes and all existing tests remain green.
- License, security policy, owners, feature lifecycle, and supported-platform statement exist.
- Baseline review, coverage, Traci, Fract, dependency, and self-lint reports are archived in CI.
- Publication and generated-execution features are flagged experimental.

### Phase 1 — eliminate unsafe external actions (weeks 1-6)

Scope: SEC-01, SEC-02, SEC-03, and UX-01 confirmation flows.

Status (2026-08-26): **Complete.** The implementation and verification evidence is recorded in `docs/PHASE1_COMPLETION.md`.

Exit gate:

- Tape execution uses a typed allowlist and sandbox.
- Telegram is deny-by-default.
- Identity is credential-bound; actor strings are metadata, never authority.
- Approval and publication require exact revision confirmation and destination binding.
- Network, process, image, and input resource limits are enforced.
- Independent security review has no unresolved critical/high finding.

### Phase 2 — make findings trustworthy (weeks 4-11)

Scope: DET-01, DET-02, DET-03, and BRD-01.

Exit gate:

- Versioned evaluation corpus is running in CI.
- Precision >=95%, recall >=90%.
- Incomplete scans cannot report clean success.
- Normalized, versioned scores remain meaningful at monorepo scale.
- Brandi self-lint is at least 80 and trending toward 90 without blanket exclusions.

### Phase 3 — harden state and architecture (weeks 8-16)

Scope: REL-01, REL-02, REL-03, ARC-01, ARC-02, and ARC-03.

Exit gate:

- Transactional state store and migrations are in place.
- Concurrency and crash tests show no lost or invalid transitions.
- Traci has zero criticals/errors.
- Fract health >=90%.
- Go consumes generated schemas and rule metadata.
- Recovery runbook passes a tabletop and automated fault exercise.

### Phase 4 — qualify enterprise operation (weeks 13-21)

Scope: QAT-01, QAT-02, QAT-03, UX-02, PLT-01, and BRD-02.

Exit gate:

- Coverage thresholds and end-to-end suites pass.
- Scale and resource objectives pass on controlled runners.
- Supported platform matrix passes native tests.
- Accessibility and operator-usability reviews pass.
- SLOs, dashboards, alerts, backup/restore, and support escalation are exercised.

### Phase 5 — release candidate and GA (weeks 20-26)

Scope: PLT-02 completion, penetration testing, documentation freeze, migration rehearsal, and launch qualification.

Exit gate:

- Two release candidates complete a minimum two-week soak with no critical/high incident.
- Signed artifacts, SBOM, provenance, checksums, upgrade, rollback, and migration instructions are published.
- RustSec/Go vulnerability, license, secret, and artifact scans pass.
- Fifty-factor review >=220/250 with no 0/1 scores.
- Product, engineering, security, SRE, and support owners sign the GA decision record.

## Quality gates by pull request

Every pull request should eventually enforce:

1. Rust and Go formatting.
2. Clippy with warnings denied, rustdoc warnings denied, Go vet, and static analysis.
3. Unit, integration, contract, property, and selected fault-injection tests.
4. Changed-lines coverage and module floor checks.
5. Traci production-only policy.
6. Fract architectural budgets.
7. Rust and Go dependency vulnerability and license policy.
8. Secret scanning and generated-artifact detection.
9. Brandi corpus precision/recall and self-lint gates.
10. Deliver acceptance contract.
11. Reproducibility and schema compatibility checks where affected.

No gate may be waived silently. Exceptions require an owner, reason, risk classification, compensating control, and expiry date.

## Operational objectives

Set final service levels after Phase 4 measurement. Initial objectives:

- Local lint availability: 99.9% successful completion for valid, readable repositories.
- Daemon detection latency: p95 under 3 seconds after filesystem quiescence.
- Durable state recovery: RPO 0 for committed local transitions; RTO under 15 minutes with documented restore.
- External delivery audit completeness: 100% of attempts and denials recorded with correlation ID.
- Security response: critical acknowledgement under 4 hours, mitigation plan under 24 hours.
- Release rollback: documented and rehearsed within 30 minutes.

## Programme metrics

Report weekly:

- Open critical/high security findings and median remediation age.
- Corpus precision, recall, and top false-positive/negative clusters.
- Self-lint score and genuine versus false-positive findings.
- Test coverage overall and for critical modules.
- Traci diagnostics by rule and module.
- Fract health, entropy, cycles, and duplication.
- Queue conflicts, failed transitions, retries, ambiguous deliveries, and recovery time.
- Cold/incremental scan latency, peak RSS, and cache hit rate.
- CI pass rate, flaky-test rate, build time, artifact size, and reproducibility rate.
- Documentation task completion and support exercise results.

## Principal risks

### Scope expansion

Risk: new integrations displace safety and precision work.

Control: feature freeze through Phase 1; every exception requires programme and security approval.

### Scanner rewrite regression

Risk: AST adapters improve precision but lose unsupported-language coverage.

Control: versioned corpus, explicit fallback mode, per-language metrics, and staged rollout.

### State migration loss

Risk: JSONL-to-database migration corrupts or duplicates approvals and outbox records.

Control: immutable backup, dry-run migration report, row-count/hash reconciliation, rollback rehearsal, and dual-read validation before cutover.

### Sandbox portability

Risk: a strong Linux sandbox cannot be reproduced safely on every platform.

Control: declare execution support per platform; disable tape execution where the required isolation cannot be guaranteed.

### Enterprise process without customer validation

Risk: the programme optimizes compliance signals but misses real adoption needs.

Control: recruit design partners in Phases 2-4 and validate workflows, false-positive tolerance, deployment, and support requirements.

## Traceability to the fifty-factor review

| Factors | Review area | Closing epics |
| --- | --- | --- |
| 1-5 | Product clarity, differentiation, audience, scope, maturity | GOV-01, GOV-02, BRD-02 |
| 6-10 | Brief/guidelines, validation, rule breadth, configuration, scoring | DET-02, DET-03, BRD-01 |
| 11-15 | Surface breadth, precision, ignores, parsing, scan integrity | DET-01, DET-02 |
| 16-20 | CLI, consistency, JSON, TUI, interaction safety | UX-01, UX-02, ARC-02 |
| 21-25 | Rust/Go tests, coverage, static quality, integration tests | QAT-01, QAT-02, PLT-02 |
| 26-30 | Modularity, entropy, contracts, dependencies, build footprint | ARC-01, ARC-02, ARC-03, PLT-02 |
| 31-35 | Secrets, command safety, paths, publication, resource limits | SEC-01, SEC-02, SEC-03 |
| 36-40 | Daemon, durability, concurrency, observability, recovery | REL-01, REL-02, REL-03 |
| 41-45 | Scan/assets/watch performance, portability, scale | QAT-03, PLT-01, SEC-03 |
| 46-50 | Documentation, metadata, VCS/CI, releases, self-coherence | GOV-01, PLT-02, BRD-01, BRD-02 |

## Immediate first ten actions

1. Create the independent repository and protect its default branch.
2. Freeze new external-action and generated-execution features.
3. Complete a formal threat model for tape, Telegram, ADB, TUI approval, and durable state.
4. Disable tape rendering until the allowlist/sandbox boundary exists.
5. Make Telegram reject an empty allowlist and remove authority from caller-supplied actor fields.
6. Add exact-ID confirmation to TUI approval and bind approvals to destination.
7. Make formatting, current tests, Clippy, rustdoc, Traci baseline, audit, and Deliver visible in CI.
8. Create the labeled surface-extraction corpus and baseline precision/recall.
9. Write the transactional-state design and JSONL migration plan.
10. Assign owners, estimates, and decision records to every epic in this roadmap.

## Definition of enterprise-grade complete

Enterprise-grade is achieved when Brandi is safe to operate, predictable under concurrency and failure, measurable at scale, supportable by people who did not build it, and conservative at every external boundary. Completion is demonstrated by the exit criteria and evidence above, not by feature count or a successful build alone.
