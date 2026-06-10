# PasteForward Remotion Demo

This package renders the README demo video. It is intentionally isolated from
the Rust CLI build and release packaging.

## Dependencies

Pinned versions were selected with a 7-day publication buffer:

- `remotion` and `@remotion/cli` `4.0.470`, published 2026-05-31
- `react` and `react-dom` `19.2.7`, published 2026-06-01
- `zod` `4.3.6`, published 2026-01-22
- `@types/react` `19.2.6`, published 2025-11-18
- `@types/react-dom` `19.2.3`, published 2025-11-12
- `typescript` `5.9.3`, published 2025-09-30

## Commands

```sh
pnpm install --frozen-lockfile
pnpm check
pnpm render:readme
pnpm render
```

Generated working output goes under `out/`. The README asset is written to
`../../docs/assets/`.
