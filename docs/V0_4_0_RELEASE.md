# Irosh v0.4.0 Release Checklist

> **Target:** Tag `v0.4.0` and publish to GitHub releases + crates.io.
>
> **Status:** In progress — code at `0.4.0` in `Cargo.toml`, documentation synced in this release prep.

---

## Release Blockers

These must pass before tagging:

| Step | Command / Action | Status |
|------|------------------|--------|
| Build | `cargo check --workspace` | Required on Windows, Linux, macOS |
| Tests | `cargo test --workspace` | All non-ignored tests must pass |
| Clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Zero warnings |
| Format | `cargo fmt --all --check` | Clean |
| CHANGELOG | `CHANGELOG.md` has `[0.4.0]` entry | Done |
| README | Version references updated | Done |
| Version | `Cargo.toml` + `cli/Cargo.toml` = `0.4.0` | Done |

---

## Platform Verification

### Windows (`WINDOWS-CHECKLIST.md`)

Run in PowerShell from repo root:

```powershell
cargo check --workspace
cargo test --workspace --quiet
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

Manual checks:

- [ ] Interactive shell session connects and disconnects cleanly
- [ ] Terminal resize during interactive session forwards to remote PTY
- [ ] `put`/`get` transfer works; resize during transfer does not break session
- [ ] `irosh system install` / `start` / `status` / `stop` (if testing service mode)
- [ ] Paths with spaces: `irosh put "C:\My Files\test.txt"`

### Linux

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

Manual checks:

- [ ] `irosh system install` / `start` / `status` / `stop` via systemd
- [ ] Wormhole pairing end-to-end
- [ ] File transfer (single file + recursive directory)

### macOS

Same commands as Linux. Verify launchd service install/start/stop if applicable.

---

## What's In v0.4.0

See [CHANGELOG.md](../CHANGELOG.md) for the full list. Highlights:

- Windows static CRT binary and job-object child cleanup
- Storage, config, error, and session unit test coverage
- Public-key auth rate limiting parity with password auth
- `SecretString` for credentials; session API concurrency improvements
- Transfer resize forwarding; release CI hardening
- Windows install/uninstall script improvements

---

## Explicitly Deferred to v0.5.0

These were considered for v0.4 but are scoped for the next minor release:

| Item | Document |
|------|----------|
| Session idle timeout | [V0_5_0_ROADMAP.md](V0_5_0_ROADMAP.md) |
| Auth rate-limit persistence across daemon restarts | [V0_5_0_ROADMAP.md](V0_5_0_ROADMAP.md) |
| Config export/import | [V0_5_0_ROADMAP.md](V0_5_0_ROADMAP.md) |
| Remote port forward CLI (`-R`) | [V0_5_0_ROADMAP.md](V0_5_0_ROADMAP.md) |
| PR/push CI workflow | [V0_5_0_ROADMAP.md](V0_5_0_ROADMAP.md) |

---

## Tagging & Publishing

When all blockers are green:

```bash
# Ensure clean tree
git status

# Tag (adjust date if needed)
git tag -a v0.4.0 -m "Release v0.4.0"

# Push tag to trigger release workflow
git push origin v0.4.0
```

The [release workflow](../.github/workflows/release.yml) builds 5 targets and runs quality gates on Linux x86_64.

### crates.io

```bash
cargo publish -p irosh
cargo publish -p irosh-cli
```

---

## Known Limitations (document, don't block release)

- 4 ignored/flaky tests (wormhole rendezvous, exec verify, 2 Windows ConPTY exec tests)
- `config export` / `config import` CLI stubs
- Remote forward exists in library API but not wired in CLI
- `docs/improvements-audit.md` has open medium-priority items (key zeroization, PATH cache, remaining `.ok()` swallows)
- **Windows CWD resolution**: `~get`/`~put` with relative paths on Windows may silently fall back to home directory. The PEB-based CWD tracking is fragile. Use absolute paths on Windows for reliable file transfers.

---

## Related Documents

- [WINDOWS-CHECKLIST.md](../WINDOWS-CHECKLIST.md)
- [V0_5_0_ROADMAP.md](V0_5_0_ROADMAP.md)
- [PROJECT_ASSESSMENT.md](PROJECT_ASSESSMENT.md)
- [pre-v0.4.0-audit.md](pre-v0.4.0-audit.md)
- [handoff-linux.md](handoff-linux.md)
