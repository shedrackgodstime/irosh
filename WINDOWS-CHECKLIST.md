# Windows Verification Checklist

## Instructions for the Windows AI

1. **Pull the repo** and run these commands in order.
2. If **any step fails**, paste the full error output back to the main AI.
3. If **all steps pass**, say `WINDOWS: GREEN` and include the test summary.

---

## 1. Build check

```powershell
cargo check --workspace 2>&1
```

## 2. Full test suite

```powershell
cargo test --workspace --quiet 2>&1
```

## 3. Clippy

```powershell
cargo clippy --all-targets --all-features -- -D warnings 2>&1
```

## 4. Formatting

```powershell
cargo fmt --all --check 2>&1
```

---

## Windows-specific things to verify manually

### 4. Resize during transfers
Open a shell session, start a file transfer (`put`/`get`), resize the terminal window while the transfer is running. The transfer should continue without error and the remote PTY should receive the new size.

### 5. Windows Service
If applicable, test:
```powershell
irosh service install
irosh service start
irosh service status
irosh service stop
```

### 6. ConPTY resize polling
The Windows resize polling loop runs every 500ms. Verify that interactive shell sessions correctly detect and forward terminal resizes by resizing the window during an `irosh` interactive session.

### 7. Path handling
Test upload/download with paths containing:
- Spaces: `irosh put "C:\My Files\test.txt"`
- Mixed separators: `irosh put ./local.txt remote:`

---

## Known cross-platform changes in this session

| Change | File(s) |
|--------|---------|
| `Credentials.password` → `SecretString` (zeroized on drop) | `src/auth.rs`, `src/client/connect.rs`, `src/server/tests.rs` |
| `Session.channel` → `tokio::sync::Mutex` (double-checked locking) | `src/client/mod.rs`, `src/client/connect.rs`, `src/client/tests/mod.rs` |
| `resize`, `send`, `eof` take `&self` (was `&mut self`) | `src/client/mod.rs` |
| Upload/download forward resize events to remote PTY | `cli/src/commands/connect/transfer.rs` |
| Rate-limiting now applies to public key auth failures | `src/auth.rs` |
| `tracing::warn!` added to 4 silent error swallows | `src/server/handler/pty.rs`, `src/client/transfer/files/download.rs`, `src/client/connect.rs` |
| `chrono` made optional behind `server` feature | `Cargo.toml` |

---

## If anything fails

Paste the **full command output** (not just the error) back to the main AI. The most likely Windows-specific issues are:

- `portable-pty` ConPTY backend differences
- Path separator handling in tests
- `#[cfg(windows)]` vs `#[cfg(unix)]` attribute mismatches
- ANSI escape sequence differences in CLI output
