# Brandi command and safety reference

Use `--path <DIR>` to target the intended project root. The display forms `brandi brief [PATH]` and `brandi guidelines [PATH]` use an optional positional target and otherwise discover the nearest Brandi project from the current directory. Verify exact syntax with `brandi <command> --help` when the installed version differs from this reference.

Every leaf accepts global `--format human|json|jsonl`, `--color`,
`--animation`, `--fidelity`, `--unicode`, `--width`, `--quiet`, and `--verbose`
controls. Machine formats never contain ANSI or animation. Use JSONL for the
foreground daemon and Telegram supervisor because their output is unbounded.

## Read-only inspection and planning

| Command | Purpose |
| --- | --- |
| `brandi brief [PATH]` | Display the nearest associated project's identity and audience brief. |
| `brandi guidelines [PATH]` | Display the nearest associated project's voice, visual, prohibited-language, and rule guidelines. |
| `brandi brief validate --path <DIR>` | Validate brief YAML and semantic constraints. |
| `brandi guidelines validate --path <DIR>` | Validate guidelines YAML and semantic constraints. |
| `brandi scan [--path <PROJECT_ROOT>] [--if-uninitialized prompt\|init\|skip]` | Run the transparent five-milestone scan. Without a path, the target project is the nearest configured root or project marker at or above cwd; an explicit path must be that top-level root. A missing `.brandi/` prompts interactive users to init or skip the target. Machine and redirected runs must choose `init` or `skip` explicitly and never block. Dreamseq delivery includes only project-local or ancestor `.dreams` files whose `project` field or canonical path reference matches the target project. |
| `brandi check <FILE> --path <DIR>` | Lint one file in project context. |
| `brandi lint --path <DIR> --format human\|json\|jsonl` | Lint the full project. `--fail-under N` and `--strict` add gates. |
| `brandi propose --path <DIR> --budget N` | Print bounded, semantic-priority-ranked changes without editing files. Findings carry provenance/audience significance; low-relevance operational machinery is not proposal-eligible. Operations distinguish replacements, rewrites, and structural edits. Colour changes show exact RGB swatches plus auditable palette-selection metadata. Typography is an opportunity with a preferred or style-derived family and inline CSS/HTML specimen. |
| `brandi assets list --path <DIR>` | List discovered image assets. |
| `brandi assets check <IMAGES...> --kind auto\|social-card\|thumbnail\|icon --path <DIR>` | Analyze dimensions, palette proximity, and whitespace. |
| `brandi social graph --path <DIR>` | Render capability, narrative, audience, and format relationships. |
| `brandi social plan --path <DIR> [--segment NAME]` | Derive a content plan from the brief. |
| `brandi social --path <DIR>` | Open the project-linked social account TUI. Tokens are masked and session/environment-only; account metadata is treated as both source evidence and public brand surfaces. |
| `brandi social accounts list\|connect\|disconnect --path <DIR>` | Manage the machine-readable account registry. Connect stores only a credential environment-variable reference; disconnect requires an exact account-id confirmation. |
| `brandi daemon status --path <DIR>` | Inspect watch-daemon state. |
| `brandi promotion stats\|plan\|milestones\|queue --path <DIR>` | Inspect metrics, plans, release evidence, or approval queue. `milestones` may append idempotent local drafts from Kaptaind indexes. |
| `brandi telegram status [--config FILE]` | Validate supervisor configuration and worker readiness. |
| `brandi social adb status --path <DIR>` | Inspect configured targets and connected devices. |
| `brandi tape generate --path <DIR> --goal <TEXT> --preset <NAME>` | Preview a tape plan when `--output` is omitted. |
| `brandi tape validate <FILE> --path <DIR>` | Run Brandi safety checks and native `vhs validate`. |

## Local writes or process changes

| Command | Effect |
| --- | --- |
| `brandi init --path <DIR>` | Creates missing `.brandi/` brief and guidelines files; never overwrites existing ones. |
| `brandi direct [--path <DIR>]` | Generates missing social-card/thumbnail SVGs with Vasilis and a validated VHS demo tape when requested by the guidelines. Existing asset classes are preserved. |
| `brandi reshoot [--path <DIR>]` | Rewrites discovered image palettes, stylesheet colour tokens, and non-code documentation surfaces against the brief and guidelines. |
| `brandi direct\|reshoot --portfolio [--path <ROOT>]` | Applies the command to every valid Brandi project beneath `HOME`, or beneath the designated root. |
| `brandi daemon start\|stop --path <DIR>` | Starts or stops a background process and updates `.brandi/state/`. |
| `brandi daemon run --path <DIR>` | Runs the watcher in the foreground and writes reports/history. |
| `brandi tape generate ... --output demos/<FILE>.tape` | Writes a confined tape draft; `--force` permits replacement. |
| `brandi tape render <FILE> --path <DIR>` | Validates, then invokes VHS rendering. |
| `brandi promotion approve\|reject <ID> --path <DIR>` | Changes the durable state of an immutable queued revision. |

## Network, device, or publication boundaries

| Command | Boundary |
| --- | --- |
| `brandi promotion sync --path <DIR>` | Replays the durable outbox to configured Padagonia. |
| `brandi telegram run [--config FILE]` | Starts the Telegram hub or fleet and may answer or publish through configured chats. |
| `brandi social adb wifi pair\|connect\|disconnect\|bridge ...` | Changes Android Debug Bridge connectivity. Prefer pairing codes through the configured environment variable, not command arguments. |
| `brandi social adb stage <ID> --target <NAME> --path <DIR>` | Opens an approved revision in an allowlisted Android app composer; does not publish. |
| `brandi social adb publish <ID> --target <NAME> --confirm <ID> --path <DIR>` | Stages and taps an allowlisted publish control after exact confirmation. |

## Brief and guidelines map

Brief (`brandi brief validate`):

- `identity.yaml`: product name, tagline, mission, archetypes, canonical terminology, former names.
- `audience.yaml`: audience segments and capability-to-narrative mappings.

Guidelines (`brandi guidelines validate`):

- `voice.yaml`: traits, signal words, heading style, exclamation policy, error-message constraints.
- `visual.yaml`: palette, color tolerance, typography notes, asset dimensions and output paths, whitespace floor.
- `prohibited.yaml`: forbidden language categories and category severities.
- `rules.yaml` (optional): per-rule `off`, `info`, `warning`, or `error` overrides.

Other: `promotion.yaml`, `reasoning.yaml`, `social-adb.yaml`: optional operational integrations; inspect without exposing secrets.

## Lint exit codes and scoring

- Exit `0`: command succeeded and any lint gates passed.
- Exit `1`: lint score fell below `--fail-under` or `--strict` found an error.
- Exit `2`: usage or runtime failure such as invalid arguments, missing brief/guidelines, or unreadable input.

The score begins at 100. Finding severity is weighted by detector confidence, audience exposure, and provenance-adjusted semantic significance, then normalized against the weighted corpus. Use the JSON report as evidence; do not infer a perfect score from exit `0` when no gate was configured.
