# PasteForward Agent Instructions

## Source Of Truth

Follow these docs first:

1. `docs/contracts.md`
2. `docs/security.md`
3. `docs/architecture.md`

Do not expand scope beyond the v0 contract without updating those docs first.

## Product Scope

PasteForward makes image paste work in Claude Code and Codex over SSH.

It forwards local image clipboard changes to remote GUI clipboards over SSH.

## Non-Goals

Do not add:

- pipe-to-shell installers
- remote fetched scripts
- package-manager auto-install
- auto-update
- telemetry
- clipboard text sync
- clipboard text history
- headless Linux path injection without an accepted design

## Supply Chain

Keep dependencies small. Add a dependency only when it removes real complexity.

When adding or updating dependencies, record the chosen version and publish date
in `docs/security.md`. Prefer versions published at least 7 days ago.

Use `cargo --locked` in verification paths.

## Verification

For code changes, run:

```sh
make verify
```

For docs-only changes, tests are optional unless contracts changed.
