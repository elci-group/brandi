---
name: brandi
description: Use Brandi's brand-coherence CLI and daemon to create or validate a `.brandi/` brand brief and guidelines, discover user-facing surfaces, lint copy and colors, inspect assets, propose dry-run revisions, plan social narratives, operate approval queues, and diagnose identity drift. Trigger when the user mentions Brandi, brand coherence, brand briefs or guidelines, identity drift, outward-facing product language, brand linting, Brandi CI gates, or asks to work on the Brandi repository itself.
---

# Brandi

Use Brandi as a brand-governance layer. Treat the `.brandi/` brief and guidelines as the project's constitution and distinguish read-only inspection from commands that write state, start processes, access networks, or publish content.

## Start safely

1. Resolve the project root from the user's scope. Pass `--path <root>` instead of relying on the current directory when there is any ambiguity.
2. Resolve the executable in this order: `brandi` on `PATH`, `<brandi-repo>/target/release/brandi`, then `<brandi-repo>/target/debug/brandi`. Build it only when the user asked for implementation or running Brandi requires it.
3. Inspect before changing anything:

   ```bash
   brandi brief validate --path <root>
   brandi guidelines validate --path <root>
   brandi scan --path <root>
   brandi lint --path <root> --format json
   ```

4. Separate tool failures from brand findings. Exit `1` from `lint` means a configured quality gate failed; exit `2` means a runtime or usage error.
5. Preserve user changes. Never rewrite a surface solely from a lint message without reading its surrounding context and the relevant brief/guidelines files.

Read [references/commands.md](references/commands.md) when selecting a less common command or evaluating its side effects.

## Choose the workflow

### Audit identity drift

- Run `brief validate` and `guidelines validate`, then `scan`, then `lint --format json`.
- Use JSON for parsing, automation, comparisons, and evidence-backed summaries. Use human output only for interactive display.
- Report the coherence score, errors, warnings, affected surfaces, and rule IDs. Prioritize error-severity findings and repeated drift across surfaces.
- Run `brandi propose --budget <N>` when concrete alternatives would help. Proposals are dry-run suggestions; review them rather than claiming files were changed.
- Run `brandi direct` when the user wants missing assets generated from the guidelines. It preserves existing asset classes and uses Vasilis plus VHS validation.
- Run `brandi reshoot` only when the user explicitly asks to revise existing visual and non-code textual assets; it mutates those surfaces. Add `--portfolio` to target every valid Brandi project beneath `HOME`, or pair it with `--path <root>`.

### Create or repair a brief/guidelines

- Run `brandi init --path <root>` only when the user asked to initialize Brandi. It is idempotent and does not overwrite existing brief/guidelines files.
- Edit `.brandi/identity.yaml`, `voice.yaml`, `visual.yaml`, `audience.yaml`, and `prohibited.yaml` from evidence supplied by the user or already present in the project.
- Use `.brandi/rules.yaml` only for deliberate per-rule severity overrides. Prefer fixing genuine drift over suppressing rules.
- Re-run `brief validate`/`guidelines validate` after every edit, then lint representative surfaces or the full project.

### Fix brand findings

1. Read the finding, its rule ID, the surrounding source, and the relevant brief/guidelines field.
2. Preserve technical meaning, factual claims, accessibility, and executable syntax.
3. Make the smallest coherent revision. Do not force mission language into every surface or replace precise terms with vague branded prose.
4. Run `brandi check <file> --path <root>` for focused feedback.
5. Run the original lint gate again. Summarize before/after scores and any remaining findings without overstating causality.

### Add Brandi to continuous integration

- Start with `brandi lint --fail-under <score>` and add `--strict` only when error-severity findings should fail immediately.
- For gradual adoption, choose an evidence-based initial threshold and raise it intentionally.
- Do not redirect JSON into a tracked path unless the user requested an artifact.

### Inspect assets and narratives

- Use `assets list` before checking discovered images.
- Use `assets check ... --kind auto` unless the intended format is known. Treat typography, composition, and message clarity as manual-review items.
- Use `social graph` to inspect capability-to-audience mappings and `social plan` to derive content ideas. These commands plan; they do not publish.

## Guard stateful and external actions

Treat these as distinct trust boundaries:

- `daemon start`, `daemon stop`, and `daemon run` change process state.
- `promotion sync` accesses the configured Padagonia service.
- `promotion approve` and `promotion reject` change durable approval state.
- `telegram run` starts external bot activity.
- `social adb wifi pair|connect|disconnect|bridge` changes device connectivity.
- `social adb stage` opens an approved revision in an Android composer.
- `social adb publish` can tap the configured publish control and requires exact draft-ID confirmation.
- `tape render` executes VHS after validation; `tape generate --output` writes a file.

Perform these only when they are explicitly within the user's request. Inspect status, configuration, queues, or generated plans first. Never expose tokens, API keys, Telegram IDs, pairing codes, or secret-bearing configuration. Never describe approval or staging as publication.

## Develop this project

When modifying the Brandi repository:

- Read `README.md`, `src/cli.rs`, and the relevant module before changing behavior.
- Keep CLI definitions, command dispatch, README examples, and tests aligned.
- Add lint rules in `src/rules.rs` with a stable ID, surface scope, default severity, actionable suggestion where possible, and unit tests.
- Run focused tests, then `cargo test`. For TUI changes, also run the Go tests from `tui/`.
- Re-run Brandi against its own brief and guidelines when outward-facing copy, colors, docs, or CLI strings change.

## Report outcomes

Lead with what Brandi found or changed. Include commands run, gates and exit meanings, files changed, remaining manual-review items, and any stateful action taken. Do not claim coherence from a successful process exit alone; cite the score and finding counts.
