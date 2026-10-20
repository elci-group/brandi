# AGENTS.md

This file is managed by poka. Edits will be overwritten on the next `poka apply`
unless the corresponding rule is disabled in poka.toml.

Project: brandi

## Policies

<!-- poka:block:policy:security -->
- Never commit secrets, API keys, or credentials. Use environment variables or a secret manager.
<!-- /poka:block:policy:security -->
<!-- poka:block:policy:testing -->
- Run the test suite before considering a change complete.
<!-- /poka:block:policy:testing -->
<!-- poka:block:policy:documentation -->
- Keep documentation in sync with behavior changes; do not let it drift.
<!-- /poka:block:policy:documentation -->

## TempCheq

- Submit every artifact containing LLM inference calls to tempcheq before delivery: run `tempcheq <path>` on the workspace that produced the artifact, review the temperature audit, and include the findings with the artifact (`tempcheq --report` for JSON output, `tempcheq report <path>` for Markdown/HTML report files).
