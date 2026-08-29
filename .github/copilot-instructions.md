# Copilot instructions

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
