# Irosh Development Roadmap (v0.3.0 -> v1.0)

The primary goal of the v0.3.0 series is to achieve **Production Stability** and **OS-Native Integration** while maintaining the high-performance "Fat Library" architecture.

## Phase 1: Feature Parity (v0.2.x) - [COMPLETE]
Restored professional-grade CLI features and unified the library architecture.
- **Authentication Parity**: Unified authenticator with multi-mode support.
- **Terminal Fidelity**: RAII-based `TerminalGuard` and robust Windows ConPTY handling.
- **Interactive Escapes**: Full `~` command mode with history and completion.
- **Wormhole Hardening**: Pkarr-based rendezvous and rate-limiting.

## Phase 2: OS Integration (v0.3.0) - [COMPLETE]
Expanding the "Service-Oriented" nature of Irosh across all major platforms.
- **Native Service Managers**: Cross-platform installers for systemd, launchd, and Windows Task Scheduler.
- **Service Diagnostics**: Unified `system status` command with real-time daemon health.
- **Storage Hardening**: High-assurance Windows ACLs and atomic secure writes.
- **IPC Lifecycle**: Synchronized shutdown of control listeners and session tasks.

## Phase 3: Stabilization & Polish (v0.4.x) - [CURRENT]

### v0.4.0 (released)
Highlights:
- Windows static CRT binary and job-object child cleanup
- Storage, config, error, and session unit test coverage
- Public-key auth rate limiting parity with password auth
- `SecretString` for credentials; session API concurrency improvements
- Transfer resize forwarding; release CI hardening
- Windows install/uninstall script improvements

**Known Limitations (v0.4.0):**
- 4 ignored/flaky tests (wormhole rendezvous, exec verify, 2 Windows ConPTY exec tests)
- `config export` / `config import` CLI stubs
- Remote forward exists in library API but not wired in CLI
- **Windows CWD resolution**: `~get`/`~put` with relative paths on Windows may silently fall back to home directory. Use absolute paths on Windows for reliable file transfers.

### v0.4.x remaining work
- **Registry PATH cache**: `src/server/handler/pty.rs` rebuilds Windows PATH from registry on every shell spawn — cache once per daemon lifetime.
- **sys/ module tests**: Platform service code, PTY, and signal handling need unit tests.
- **CLI unit tests**: Most CLI source files (29 files) have zero unit tests.
- **Flaky test triage**: `test_wormhole_rendezvous`, `verify_exec_output`, 2 Windows ConPTY tests.

### v0.5.0 — Stabilization & Production Hardening
A release focused on closing documented gaps and hardening security.

**Security & Reliability:**
- **Session idle timeout**: Configurable inactivity timeout to close stale SSH channels.
- **Auth rate-limit persistence**: Persist `failed_attempts` counter across daemon restarts.
- **Key material zeroization**: Switch `src/storage/keys.rs` from hex `String` to zeroize-backed buffers.
- **Remaining `.ok()` swallows**: Add `tracing::warn!` to diagnostic.rs, metadata subprocess, and upload entry-skip paths.

**CI & Tests:**
- Add PR/push CI workflow (fmt, clippy, test on every commit)
- Criterion benchmarks for transfer throughput
- Triage 4 flaky tests

**New capabilities:**
- `irosh config export/import` — finish the stub
- `irosh connect -R` — remote port forward CLI (library API already exists)
- `irosh check` / `status` — enrich with NAT type, relay path, latency diagnostics

### v0.4.x Release Blockers (reference)
Before tagging any v0.4.x patch:
1. `cargo check --workspace` — green on Windows, Linux, macOS
2. `cargo test --workspace` — all non-ignored tests pass
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings` — zero warnings
4. `cargo fmt --all --check` — clean
5. `CHANGELOG.md` updated
6. `Cargo.toml` + `cli/Cargo.toml` version bumped

## Phase 4: Production Readiness (v1.0)
Finalizing the protocol and committing to API stability.
- **Stability Freeze**: Finalize the IPC and pairing protocol versions.
- **Security Audit**: Independent review of the authentication and namespace-joining logic.
- **Cloud Relay Fleet**: Deployment of a global relay network for zero-config internet traversal.

---

> [!NOTE]
> For our long-term vision and advanced P2P concepts (Collaborative SSH, Mobile support, AI agents), please see the [Future Roadmap](future_roadmap.md).
