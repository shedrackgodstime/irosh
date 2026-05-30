# Handoff: Windows Work Complete — Proceed on Linux

## What Was Done on Windows

### Build System
- **`.cargo/config.toml`** — added `target-feature=+crt-static` for `x86_64-pc-windows-msvc` so the `.exe` has zero DLL dependencies (no VC Redist needed)
- **`.github/workflows/release.yml`** — same fix in CI for the Windows build job

### PowerShell Scripts
- **`public/scripts/install.ps1`** — smart update logic: detects if irosh service is running → stops it → installs new binary → restarts it. Warns about interactive processes. Skips redundant `system install` on update.
- **`public/scripts/uninstall.ps1`** — removed spurious `schtasks /delete` (irosh uses SCM services, not scheduled tasks)

### Rust Source (Windows-specific)
- **`src/sys/windows/service.rs`** — `exe_path` now returns an error instead of silently falling back to a relative `"irosh.exe"` path
- **`src/sys/windows/pty.rs`** — added doc comment explaining `map_sig` returns `None` on Windows (no POSIX signals)

### Housekeeping
- **`Cargo.toml`** — fixed stale comment referencing non-existent `docs/architecture.md`
- **`docs/pre-v0.4.0-audit.md`** — full codebase audit (test gaps, roadmap features, TODOs)

## What Is Committed (files changed)
```
.github/workflows/release.yml
.cargo/config.toml
Cargo.toml
public/scripts/install.ps1
public/scripts/uninstall.ps1
src/sys/windows/service.rs
src/sys/windows/pty.rs
docs/pre-v0.4.0-audit.md
docs/handoff-linux.md
```

## Critical: Before Your First Build on Linux
The `Cargo.toml` version is `0.4.0`. After pulling, run:
```bash
cargo check --workspace
```
This ensures no Windows-specific changes broke the Linux build. The `+crt-static` flag is scoped to `x86_64-pc-windows-msvc` and won't affect Linux builds.

## Priority Work for Linux (from pre-v0.4.0-audit.md)

### 1. HIGHEST — Write tests for untested modules
| Module | Files | Impact |
|--------|-------|--------|
| `src/storage/` | 7 files (keys, peers, trust, shadow, config, utils) | Auth persistence, TOFU records |
| `src/sys/` | 9 files (unix, signals, service, pty) | Platform-specific code, service management |
| `src/session/` | mod.rs, state.rs, pty.rs | SSH session lifecycle |
| `src/config.rs` | 1 file | StateConfig and options |
| `src/error.rs` | 1 file | All error types |

### 2. HIGH — Complete v0.4.0 roadmap features
From `V0_4_0ROADMAP.md`:
- **Automatic Session Timeout** — idle-timeout logic to close inactive SSH channels
- **Authenticator Persistence** — persist trust tokens & rate-limit states across daemon restarts
- **Android Client Polish** — terminal resizing and input handling for Termux
- **Performance Benchmarking** — transfer speed tests over high-latency P2P links

### 3. MEDIUM — CLI test coverage
20+ CLI source files have zero tests. Only `input.rs`, `transfer.rs`, `completion.rs` are covered.

### 4. MEDIUM — Flaky integration tests
- `test_wormhole_rendezvous` — ignored: gossip flaky without relays
- `verify_exec_output` — ignored: rendezvous flaky in isolated environments
- 2 Windows tests ignored: ConPTY hangs on short-lived exec

### 5. LOW — Leftover TODOs
- `cli/src/commands/connect/transfer.rs:272,428` — "Handle concurrent resize via session handle"

## Verify Before Release
```bash
cargo clippy --workspace --all-features   # Zero warnings expected
cargo test --workspace                     # All passing
```
