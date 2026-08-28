# Contributing

## Platform

This repository targets Windows only.

## Workflow

1. Create a feature branch from `main`.
2. Make small, reviewable commits.
3. Keep v1.0.0 within the scope documented in `README.md`, `CLAUDE.md`, and `docs/V1.0.0.md`.
4. Run all quality gates before opening a PR.
5. Open a pull request against `main` and include test/build evidence.

## Minimum quality gates

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
frontend lint/test
Tauri Windows build
```

Do not commit personal browser profiles, credentials, TOTP seeds, recovery codes, runtime databases, or generated portable workspace data.

## Security

Never place secrets in logs, diagnostics, test fixtures, screenshots, issue text, or commit history.

Use synthetic test credentials only.
