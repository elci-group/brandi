# Phase 1 security controls

This document is the operator contract for Brandi’s generated execution and external-action boundary. Defaults fail closed.

## Identity and authorization

- Local identities come from the effective operating-system UID. `.brandi/authorization.yaml` maps that credential-bound UID to `viewer`, `planner`, `approver`, `publisher`, `device-operator`, or `administrator` roles.
- Telegram requires a non-empty `allowed_users` list unless an operator explicitly enables warned `public_mode`. Project roles separately grant planning, approval, and publication.
- Approval requires an exact stored draft ID or SHA-256 revision hash, an explicit destination, and a pending revision from the current project. It expires after 15 minutes.
- Delivery revalidates the stored revision hash, expiry, status, and exact destination. Replay, stale, cross-project, and cross-destination attempts are denied.
- `.brandi/state/security-audit.jsonl` is append-only and SHA-256 chained. Back it up with project state; a broken `previous_hash` chain is evidence of modification.

## Generated execution

- VHS plans contain a typed `program` plus `args`. Only the documented read-only Brandi commands pass policy.
- `tape render` first shows the normalized plan and SHA-256 confirmation. Execution requires that exact token.
- Bubblewrap is mandatory. The sandbox has no network, a read-only host, a clean environment, a private `/tmp`, only a disposable output mount, and a fixed read-only bind of the verified Brandi executable.
- One regular file up to 256 MiB is promoted into `demos/` after successful execution. Symlink escapes are rejected.
- `.brandi/state/tape-executions.jsonl` records the plan hash, OS UID, sandbox policy, Brandi and VHS digests, exact commands, timestamps, and result.

## Declared resource budgets

| Boundary | Limit |
| --- | --- |
| Source file | 4 MiB |
| Repository scan | 100,000 files, 64 directory levels |
| Asset command | 64 images |
| Encoded raster | 64 MiB |
| Raster dimensions | 8,192 pixels per side |
| Decoded raster | 40 million pixels, 256 MiB allocation |
| HTTP response | 2 MiB |
| HTTP connection / attempt / idle | 5 s / 12 s / 10 s |
| HTTP attempts | 3 within a bounded backoff loop |
| Generic subprocess output | 2 MiB per stdout/stderr stream |
| ADB subprocess | 30 s |
| VHS validation / render | 15 s / 10 min |
| Telegram projects | 16 |

## Dependency policy and YAML exception

`Cargo.lock` and `tui/go.sum` pin resolved dependency graphs. `deny.toml` denies advisories, yanked crates, wildcard dependencies, unknown registries, unknown Git sources, and unapproved licenses. Image support is restricted to PNG, JPEG, GIF, and WebP to reduce codec and advisory exposure.

The TUI requires Go 1.25.8 or newer so its standard library includes the security fixes enforced by `govulncheck`; CI resolves the exact minimum from `tui/go.mod`.

`serde_yaml 0.9` is deprecated but has no current RustSec vulnerability. It remains isolated to trusted local configuration and structured project files; external network responses use JSON. The supply-chain gate monitors it on every run. Migration to a maintained YAML implementation remains required before GA; this is a monitored exception, not an approval to expand its use.

Run `scripts/generate-supply-chain.sh` for release SBOMs and license inventories. The release workflow installs pinned generators, runs Rust and Go vulnerability/policy gates, and uploads the generated artifacts.
