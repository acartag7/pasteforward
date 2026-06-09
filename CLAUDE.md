# PasteForward Claude Instructions

Read `AGENTS.md` first. It is the source of truth for project scope,
supply-chain rules, and verification.

Use:

```sh
make verify
```

before reporting code changes as done.

Do not add installer behavior that fetches remote shell scripts, auto-installs
packages, syncs clipboard text, or adds telemetry.
