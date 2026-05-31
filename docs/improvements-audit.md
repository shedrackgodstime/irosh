# Irosh Codebase Improvement Audit

Generated: 2026-05-30
Updated: 2026-05-30 (items 1-3 completed)

## HIGH PRIORITY

### ~~Security: Unsafe blocks without `// SAFETY:` comments~~ ✅

SAFETY comments added to all unsafe blocks across the codebase:
- `src/sys/windows/job.rs` — JobObject creation, assignment, cleanup
- `src/server/shell_access.rs` — NtQueryInformationProcess FFI declaration
- `src/server/handler/pty.rs` — GenerateConsoleCtrlEvent
- `src/sys/windows/pty.rs` — `unsafe impl Send/Sync`
- `src/sys/signals.rs` — `unsafe extern "system" fn ctrl_handler`

### ~~CI: Release workflow needs hardening~~ ✅

**`.github/workflows/release.yml`**
- Added `--locked` flag on `cargo build`
- Added `cargo test`, `cargo clippy -D warnings`, and `cargo fmt --all --check` steps (Linux x86_64 only)
- Added `Swatinem/rust-cache@v2` with target-specific key

### ~~Unused dependencies~~ ✅

Removed from `Cargo.toml`:
- `data-encoding` — was not imported anywhere
- `postcard` — was not imported anywhere

---

## MEDIUM PRIORITY

### Dead code with `#[allow(dead_code)]` suppression

| File | Line | Item |
|------|------|------|
| `src/server/transfer/helpers.rs` | 73 | `UploadSink::Process` variant dead on non-Linux |
| `src/server/transfer/helpers.rs` | 262 | `DownloadSource::Process` variant dead on non-Linux |
| `src/sys/windows/job.rs` | 56 | Suppressed dead code |
| `cli/src/ui/mod.rs` | 49 | Suppressed dead code |
| `cli/src/terminal.rs` | 25, 35 | Suppressed dead code |
| `cli/src/ui/prompts.rs` | 106 | Suppressed dead code |
| `cli/src/commands/mod.rs` | 275-283 | `ConfigAction::Export`/`Import` defined but never handled |

### `.ok()` calls silently swallowing errors

16 locations where errors are discarded without logging:

| File | Line | Context |
|------|------|---------|
| `src/diagnostic.rs` | 149 | SSH version check failure silently ignored |
| `src/client/connect.rs` | 332 | Password prompter panic silently ignored |
| `src/client/transfer/files/download.rs` | 453 | File flush failure silently dropped |
| `src/server/handler/pty.rs` | 532 | Channel write failure — data loss risk |
| `src/server/handler/pty.rs` | 540 | `child.wait()` exit status lost |
| `src/transport/metadata/types.rs` | 103, 135 | Subprocess failures silently ignored |
| `src/client/transfer/files/upload.rs` | 518 | Walkdir entry error silently skipped |

**Fix**: Log warnings with `tracing::warn!` on these paths.

### Key material in non-zeroable `String`

`src/storage/keys.rs:123-127` — Secret key bytes are converted to a hex `String` for file writes. `String` cannot be securely zeroed in Rust, meaning a process memory dump could recover the key.

**Fix**: Use `zeroize` or write raw bytes directly via `atomic_write_secure`.

### Feature gate missing on `iroh` re-export

`src/lib.rs:66` — `pub use iroh;` has no `#[cfg(feature = "transport")]` gate, so `iroh` is always compiled even without the transport feature.

### No rate limiting on public key auth

`src/auth.rs:521` — `UnifiedAuthenticator::check_public_key` does not check `failed_attempts`. Only password auth is rate-limited. An attacker could brute-force public keys without triggering the rate limit.

### Performance: Registry PATH built on every shell spawn

`src/server/handler/pty.rs:196-263` — Windows PATH is reconstructed from the registry every time a shell is requested. Cache the result.

### CLI crate: 29 files, 0 unit tests

The CLI crate (`cli/src/`) has no test modules. Only `input.rs`, `transfer.rs`, and `completion.rs` have coverage via integration tests.

### `chrono` dependency is unconditional

`Cargo.toml:85` — `chrono` is always compiled but only used in `src/server/mod.rs`. Feature-gate it behind `server`.

---

## LOW PRIORITY

### v0.4.0 features not implemented

- **Session idle timeout** — Not implemented. The `Session` struct has no inactivity timeout mechanism.
- **Android polish** — No Android-specific code exists. No Android target in release workflow.
- **Performance benchmarks** — No `benches/` directory, no Criterion setup.

### Unfinished TODOs

| File | Line | Content |
|------|------|---------|
| `cli/src/commands/connect/transfer.rs` | 272 | `// TODO: Handle concurrent resize via session handle` |
| `cli/src/commands/connect/transfer.rs` | 428 | `// TODO: Handle concurrent resize via session handle` |

### Legacy code marked for potential removal

`src/transport/ticket.rs:108` — JSON fallback for legacy tickets: `// Fallback to JSON (legacy management - maybe remove later?)`

### Missing documentation

Several public API items lack doc comments:

- `src/client/mod.rs` — `ExecOutput` struct fields, `TransferProgress::percent()`
- `src/server/mod.rs` — `ActiveSession`, `SessionTracker` methods, `ActiveWormhole`, `GossipProtocol` fields
- `src/transport/transfer/types.rs` — `PutRequest::recursive`, frame types
- `src/storage/utils.rs` — `apply_secure_permissions`
- `src/server/transfer/helpers.rs` — `UploadSink`, `DownloadSource`
