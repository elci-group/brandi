# Phase 1 completion record

Phase 1 of the enterprise roadmap was completed on 2026-08-26. This record maps the exit gate to implementation and reproducible evidence.

## Exit-gate evidence

- **Typed, sandboxed generated execution:** tape scenes use a structured `program` and `args` contract. Policy permits only exact read-only Brandi commands. Rendering requires a SHA-256 confirmation and Bubblewrap; it removes network access, uses a clean environment and private temporary directory, mounts the digest-recorded Brandi executable at a fixed path, writes into a disposable mount, and promotes one bounded regular output file only after success.
- **Deny-by-default Telegram:** an empty allowlist is rejected unless warned public read-only mode is explicitly enabled. Read-only commands require the viewer role outside public mode; planning, approval, and publication have distinct role checks.
- **Credential-bound identity:** local authority derives from the effective OS UID and `.brandi/authorization.yaml`; Telegram authority derives from the authenticated Telegram user ID. Caller-supplied actor labels do not grant authority.
- **Exact approval and destination binding:** approval and rejection require exactly a draft ID and revision hash. Approval binds the current revision to one destination for 15 minutes. Delivery revalidates project, state, revision, expiry, role, and destination and rejects replay.
- **Resource limits:** source scans, traversal, images, HTTP, subprocess output, ADB operations, VHS validation/rendering, and Telegram project counts have explicit limits documented in `docs/SECURITY_CONTROLS.md`.
- **Confirmation UX:** the CLI and TUI show the affected draft, revision hash, destination, and content and require exact confirmation before promotion, ADB publication, or generated tape execution.

## Verification snapshot

- Rust: 141 tests passed; formatting, Clippy with warnings denied, and rustdoc passed.
- Go: tests and vet passed using the minimum supported Go 1.25.8 toolchain.
- Supply chain: Cargo Deny passed advisories, bans, licenses, and sources. `govulncheck` reported zero reachable vulnerabilities with Go 1.25.8.
- Adversarial tests cover command injection forms, path and symlink escape, output and timeout exhaustion, stale/replayed/cross-project/cross-destination approval, unbounded image input, and unsafe ADB content.
- The deterministic `deliver` manifest verifies required controls and both Rust and Go quality gates.

The independent automated security review found no unresolved critical or high-severity issue in the Phase 1 external-action boundary. Traci’s remaining observability diagnostics are tracked by REL-02 in Phase 3; its critical labels are hard-coded regex construction and test-code panic assertions, not reachable external-action vulnerabilities.

Live Telegram publication, physical-device ADB publication, and VHS rendering require operator-owned credentials or hardware and are intentionally excluded from unattended verification. Their authorization and execution boundaries are covered by unit tests and fail closed when prerequisites are absent.
