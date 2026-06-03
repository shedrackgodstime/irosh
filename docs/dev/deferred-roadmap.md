# Deferred Roadmap — items not yet tackled

## How to use this file
Each section is a self-contained chunk of work. When you're ready to pick something up,
read the section, do the work, then check it off.

---

## 1. Repo Governance (first impression)

- [ ] **CONTRIBUTING.md** — How to set up the dev environment, run tests, submit PRs, code style guide
- [ ] **SECURITY.md** — How to report vulnerabilities, expected response time, PGP key if applicable
- [ ] **CODE_OF_CONDUCT.md** — Standard Rust or Contributor Covenant CoC
- [ ] **README badges** — Add: `CI (main)`, `cargo audit`, `code coverage`, `crates.io version`, `docs.rs`, `MSRV`, `license`
- [ ] **Issue templates** — `.github/ISSUE_TEMPLATE/bug_report.md`, `feature_request.md`
- [ ] **PR template** — `.github/PULL_REQUEST_TEMPLATE.md` with checklist

## 2. Architecture Documentation

- [ ] **Create `docs/adr/`** — Architecture Decision Records directory
  - `0001-use-tokio.md` — Why tokio was chosen
  - `0002-transfer-protocol.md` — Why custom framed protocol over SSH
  - `0003-auth-model.md` — TOFU + password + wormhole
  - `0004-iroh-p2p.md` — Why iroh for NAT traversal
- [ ] **`docs/architecture.md`** — High-level architecture diagram and component descriptions
- [ ] **`docs/security.md`** — Threat model, trust model, crypto usage

## 3. Fuzzing

- [ ] **Seed corpus for `fuzz_ticket`** — Create `fuzz/corpus/fuzz_ticket/` with valid ticket strings, edge cases, JSON fallback
- [ ] **Seed corpus for `fuzz_ipc`** — Create `fuzz/corpus/fuzz_ipc/` with valid IPC commands, edge cases, malformed JSON
- [ ] **Seed corpus for `fuzz_transfer_frame`** — Create `fuzz/corpus/fuzz_transfer_frame/` with valid frames for all 20+ kinds
- [ ] **CI fuzz run** — Add `cargo fuzz run fuzz_ticket -- -max_total_time=300` as a CI step
- [ ] **Regular fuzz campaign** — Nightly or weekly long-running fuzz (not in CI)

## 4. Release Process

- [ ] **Install `cargo release`** and configure `[package.metadata.release]` in Cargo.toml
- [ ] **Pre-release checklist** — Run full CI, audit, deny, miri, fuzz before every release
- [ ] **Changelog** — Add entries for v0.4.0 and v0.5.0
- [ ] **Automated tagging** — GitHub Actions to auto-tag on release PR merge
- [ ] **crates.io publish** — `cargo publish` step in release workflow
- [ ] **GitHub Release** — Create release notes from changelog

## 5. Remaining Full-Pedantic Items (after high-signal)

- [ ] **`format!("{}", var)` → just `var`** — ~146 instances of `format!` that can be simplified (mechanical, safe to bulk-edit)
- [ ] **Items after statements** — ~18 items that should move before statements in function bodies
- [ ] **`match` → `if let`** — ~15 single-arm matches that should be `if let`
- [ ] **Casts: `u16 as u32` → `From`** — ~13 instances of `x as u32` where `u32::from(x)` is safer
- [ ] **Redundant closures** — ~12 closures that can be simplified (e.g., `|| foo()` → `foo`)
- [ ] **Docs missing backticks** — ~13 doc comments missing `` ` `` around code identifiers
- [ ] **Used underscore-prefixed binding** — ~7 variables prefixed with `_` but actually used
- [ ] **Unused `async`** — ~6 functions marked `async` with no `.await` (remove `async`)
- [ ] **Matching over `()`** — ~6 places matching `()` explicitly (just call the fn)
- [ ] **Identical match arm bodies** — ~4 match arms with identical bodies (consolidate)
- [ ] **`map(...).unwrap_or(false)` on Result** — ~4 places where `.is_ok()` is clearer
- [ ] **`let...else` rewrites** — ~4 places where `let...else` is more idiomatic

## 6. Quality Checklist Remaining Items

From `docs/quality-check-list.md`, still pending:
- [ ] `#![deny(variant_size_differences)]` (enum optimization)
- [ ] `#![deny(unused_import_braces)]`
- [ ] `#[must_use]` on all non-`Result` return types that have side effects
- [ ] All `match` arms handle every variant (no wildcard silencing)
- [ ] Public API soundness reviewed (cannot violate invariants from safe code)
- [ ] `Send + Sync` bounds verified on all public types
- [ ] Cancellation safety reviewed for `select!` loops
- [ ] No blocking calls in async context
- [ ] Allocation profiling on hot paths
- [ ] `cargo outdated` reviewed for stale dependencies
- [ ] `Cargo.lock` checked into VCS and CI-verified
- [ ] MSRV declared and tested in CI
- [ ] Feature flags verified additive
- [ ] Semver compliance reviewed
- [ ] Changelog / release process documented

## 7. MSRV

- [ ] **Add MSRV CI check** — `dtolnay/rust-toolchain@1.85` job that runs `cargo check`
- [ ] **Review MSRV bump policy** — Document in CONTRIBUTING.md

## 8. Security

- [ ] **Audit `unsafe` blocks** — Review all `unsafe` in `src/server/transfer/ssh/mod.rs` for soundness
- [ ] **Constant-time comparisons** — Review password/key comparison paths for timing attacks
- [ ] **DoS review** — Payload size limits, connection rate limits, resource exhaustion
