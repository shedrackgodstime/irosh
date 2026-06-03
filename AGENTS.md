# irosh — AI Agent Guide

This file defines rules, conventions, and quality gates for AI agents (opencode, etc.) working on this project.

## Core Principle

**Every bug fix must include a test that reproduces the bug.** The test is committed alongside the fix. This guarantees the bug never regresses.

## Test Conventions

- All tests go in `#[cfg(test)] mod tests { ... }` blocks, co-located with the code they test.
- Integration tests go in `tests/` at the workspace root.
- Property-based tests (proptest) go in a separate `#[cfg(test)] mod fuzz { ... }` block.
- New features require at minimum:
  - Happy-path unit test
  - Error-path unit test  
  - Property-based invariant test (if the feature processes arbitrary input)
- Tests must use real I/O (duplex streams, temp directories) — no mocks unless unavoidable.

## Code Style

- No comments unless the code's intent is non-obvious.
- Follow existing patterns in the file you're editing.
- All public API functions must have doc comments.
- Use `tracing::instrument` on public async functions.
- Use `thiserror` for error types.
- Derive `Debug`, `Clone`, `PartialEq`, `Eq` on public types where appropriate.

## Prohibited

- Do NOT change `VERSION = 1` in the wire protocol without bumping it AND adding capability negotiation.
- Do NOT delete or rename a public API item without a deprecation period.
- Do NOT change error variant fields without updating ALL match arms.
- Do NOT commit secrets, keys, or tokens.
- Do NOT use emojis in user-facing CLI output.
- Do NOT create documentation files unless explicitly requested.

## Commit Convention

```
<type>: <brief description>

<body if needed>
```

Types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `ci`
- `fix` commits MUST include a test that reproduces the bug.
- `feat` commits MUST include tests for the new functionality.
- `chore` commits (renames, version bumps, dep updates) need no tests.

## Pre-Commit Gate

Before every commit, the following MUST pass:
```
cargo fmt --check
cargo clippy --workspace --all-features -- -D warnings
cargo hack check --each-feature --no-dev-deps
cargo test --workspace --all-features --lib   # quick run (unit tests only)
```

## Branch Strategy

- `main` is the only branch. No feature branches.
- Work is committed directly to `main`.
- If work is incomplete, use `#[ignore]` on tests and `// TODO` comments.

## CI Expectations

Every push to `main` runs:
  - `cargo fmt --check`
  - `cargo clippy --all-features -- -D warnings`
  - `cargo test --workspace --all-features`
  - `cargo deny check`
  - `cargo audit`
  - `cargo hack check --each-feature --no-dev-deps`
  - Code coverage (>= 80%)
  - Platform builds: Linux, Windows, macOS
  - Fuzz targets (30s each)

If any CI step fails, the commit must be fixed before further work.

## Files to Never Edit Without Explicit Request

- `Cargo.lock` — only changed by `cargo update` or `cargo add`
- `public/scripts/install.*` — only changed with cross-platform testing
- `public/scripts/uninstall.*` — same
- `.github/workflows/release.yml` — only when cutting a release

## State Path Resolution (Windows)

When running as SYSTEM (SSH session spawned by the Windows service), `dirs::home_dir()` returns `C:\Windows\System32\config\systemprofile`. This is WRONG for finding state files.

The correct state path on Windows is baked into the service registration via `--state`. Child processes (SSH shells) must:
1. Check for `IROSH_STATE` environment variable.
2. Check common user profile paths (`C:\Users\*\AppData\Local\irosh\server`).
3. Fall back to the default detection (but NOT SYSTEM's profile).

## Input Engine Invariants (process_local)

1. `engine.mode` must always be `Remote` or `LocalEdit` — never any other value.
2. When `active_line` is `Some`, `cursor <= line.len()`.
3. `process_local` must never panic on any input (fuzz-tested).
4. When `~` enters escape mode, the remaining bytes in the same buffer MUST NOT leak to `to_remote`.

## Protocol Version

- `VERSION = 1` in the wire protocol.
- New frame kinds can be added but old peers will reject them (no version negotiation).
- If you add frame kinds, add a test that old-decoder rejects them with `UnsupportedKind`.
