# Pre-v0.4.0 Codebase Audit

> **Note (2026-05-31):** Several items below were resolved before the v0.4.0 tag. Session idle timeout and authenticator persistence are deferred to [V0_5_0_ROADMAP.md](V0_5_0_ROADMAP.md). See [V0_4_0_RELEASE.md](V0_4_0_RELEASE.md) for release status.

## Status: Good ✅
- No `todo!()` or `dbg!()` in production code
- 93% of `unwrap()` calls are in tests; production uses `expect()` with messages
- Zero-warning Clippy compliance
- Transport layer well-tested (18 tests)
- Auth module well-tested (10 tests)
- Fuzz/proptest infrastructure exists for input engine & completion

## Needs Work

### 1. ~~`storage/` module — 7 files, zero tests~~ ✅ RESOLVED

`src/storage/` now has comprehensive unit tests (keys, peers, trust, shadow, config, utils).

### 2. `sys/` module — platform service code, limited tests
`src/sys/` (signals.rs, service.rs, unix/*, windows/*)
- No tests for platform-specific service management, PTY, signal handling
- **Priority: Critical** — bugs here break installation/service on specific OS

### 3. ~~`config` + `error` + `session` modules — zero tests~~ ✅ RESOLVED

Unit tests added for `src/config.rs`, `src/error.rs`, and `src/session/`.

### 4. v0.4.0 roadmap features — DEFERRED to v0.5.0
From [V0_5_0_ROADMAP.md](V0_5_0_ROADMAP.md) (formerly planned for v0.4):
- **Automatic Session Timeout** — idle-timeout logic to close inactive SSH channels
- **Authenticator Persistence** — persist trust tokens & rate-limit states across daemon restarts
- **Android Client Polish** — terminal resizing and input handling for Termux
- **Performance Benchmarking** — transfer speed tests over high-latency P2P links
- **Priority: High for v0.5.0**

### 5. CLI crate 90% untested
20+ CLI source files have zero tests. Only `input.rs`, `transfer.rs`, `completion.rs` have coverage.
- Files without tests: main.rs, context.rs, display.rs, output.rs, terminal.rs, commands/{mod.rs, wormhole.rs, trust.rs, system.rs, peer.rs, passwd.rs, identity.rs, host.rs, dashboard.rs, config.rs, check.rs, connect/{mod.rs, session.rs, prompt.rs, history.rs, editor.rs, tunnels.rs}}, ui/{mod.rs, theme.rs, prompts.rs, feedback.rs}
- **Priority: Medium**

### 6. Flaky integration tests
- `test_wormhole_rendezvous` — ignored: gossip flaky without relays
- `verify_exec_output` — ignored: rendezvous flaky in isolated environments
- 2 Windows tests ignored: ConPTY hangs on short-lived exec in CI
- **Priority: Medium**

### 7. Leftover TODOs
- `cli/src/commands/connect/transfer.rs:272,428` — "Handle concurrent resize via session handle"
- **Priority: Low**
