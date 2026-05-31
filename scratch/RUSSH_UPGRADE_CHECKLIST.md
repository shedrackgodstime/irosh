# Russh 0.55 → 0.61.1 Migration Checklist

**Target**: `russh = { version = "0.61.1", optional = true, default-features = false, features = ["ring"] }`
**Branch**: `upgrade/russh-0.61.1`

---

## Phase 0: Preparation

- [ ] Create branch `upgrade/russh-0.61.1` from `main`
- [ ] Read `temp/ref/russh/RESEARCH.md` — updated analysis
- [ ] Read `scratch/RUSSH_UPGRADE_ANALYSIS.md` — full improvement breakdown
- [ ] Read `docs_dev/rust-skills/AGENTS.md` — project rules
- [ ] Verify `temp/ref/russh/russh/src/lib_inner.rs` — all public API types
- [ ] Verify `temp/ref/russh/russh/src/server/mod.rs` — `server::Handler` trait
- [ ] Verify `temp/ref/russh/russh/src/client/mod.rs` — `client::Handler` trait
- [ ] Verify `temp/ref/russh/russh/src/keys/mod.rs` — `pub use ssh_key::{...}` re-exports
- [ ] Verify `temp/ref/russh/russh/Cargo.toml` — `ed25519-dalek = 3.0.0-pre.7` confirmed

---

## Phase 1: Dependency Update

### 1.1 Update root `Cargo.toml`
- [ ] Change `russh = { version = "0.55.0", ... }` → `russh = { version = "0.61.1", ... }`
- [ ] Keep `features = ["ring"]` to retain existing crypto backend
- [ ] Keep `default-features = false`
- [ ] Run `cargo update -p russh` to resolve dependencies
- [ ] Verify `cargo tree -i russh` shows `ed25519-dalek v3.0.0-pre.7` (single version)

### 1.2 Verify dependency resolution
- [ ] `grep ed25519-dalek Cargo.lock` — should see only one version (`3.0.0-pre.7`)
- [ ] `cargo check --features server,client` — must compile without errors
- [ ] `cargo check --workspace` — must compile workspace

---

## Phase 2: Config Migration

### 2.1 `src/server/mod.rs` — `ServerOptions` → `server::Config`
- [ ] Wrap `server::Config` construction in `Arc::new(...)`
- [ ] Review new fields and set as needed:
  - [ ] `auth_rejection_time_initial` — separate timing for initial "none" auth probe
  - [ ] `channel_buffer_size` — per-channel backpressure buffer
  - [ ] `event_buffer_size` — internal event buffer
  - [ ] `inactivity_timeout` — connection GC timeout
  - [ ] `keepalive_interval` — periodic keepalive
  - [ ] `keepalive_max` — max unanswered keepalives before disconnect
  - [ ] `nodelay` — TCP_NODELAY control
- [ ] Verify `Config.keys` field type — now `Vec<PrivateKey>` (check construction sites)
- [ ] Verify `Config.methods` field type — now `auth::MethodSet` (check `MethodSet::empty()` usage)

### 2.2 `src/client/connect.rs` — `client::Config`
- [ ] Wrap `client::Config` construction in `Arc::new(...)`
- [ ] Review new fields:
  - [ ] `anonymous` — anonymous auth mode
  - [ ] `gex` — DH group exchange params
  - [ ] `inactivity_timeout`
  - [ ] `keepalive_interval`
  - [ ] `keepalive_max`
  - [ ] `nodelay`
  - [ ] `channel_buffer_size`
- [ ] Update `connect_stream()` call to pass `Arc<Config>`

### 2.3 `src/client/tests/` and `src/server/tests.rs`
- [ ] Update all `Config` construction sites to use `Arc::new(...)`
- [ ] Update all `run_stream()` / `connect_stream()` calls

---

## Phase 3: Connection Function Signatures

### 3.1 `russh::server::run_stream()`
**Old signature (0.55)**: `run_stream(config, stream, handler)`
**New signature (0.61)**: `run_stream(Arc<Config>, stream, handler) -> Result<RunningSession<H>, H::Error>`
- [ ] Update `src/server/mod.rs:487` — call site
- [ ] Update `src/client/tests/mod.rs:66` — test call site
- [ ] Update any other test call sites

### 3.2 `russh::client::connect_stream()`
**Old**: `connect_stream(config, stream, handler) -> Result<Handle<H>, H::Error>`
**New**: same but `config: Arc<Config>`
- [ ] Update `src/client/connect.rs` — call site
- [ ] Update tests

---

## Phase 4: Server Handler Trait

### 4.1 New required trait methods (all have defaults — verify defaults match desired behavior)
- [ ] `auth_none()` — accept/reject "none" auth probes
- [ ] `auth_publickey_offered()` — pre-auth key probe callback (default: accept all)
- [ ] `auth_openssh_certificate()` — OpenSSH certificate auth
- [ ] `auth_keyboard_interactive()` — keyboard-interactive auth
- [ ] `authentication_banner()` — auth banner message
- [ ] `auth_succeeded()` — post-auth success callback (new `&mut Session` param)
- [ ] `channel_open_x11()` — X11 channel open
- [ ] `channel_open_direct_streamlocal()` — UNIX socket forwarding
- [ ] `channel_open_forwarded_tcpip()` — remote TCP forwarding
- [ ] `x11_request()` — X11 forwarding request
- [ ] `agent_request()` — SSH agent forwarding
- [ ] `tcpip_forward()` / `cancel_tcpip_forward()` — TCP port forwarding
- [ ] `streamlocal_forward()` / `cancel_streamlocal_forward()` — UDS forwarding
- [ ] `lookup_dh_gex_group()` — custom DH group selection

### 4.2 Changed method signatures
- [ ] `channel_open_session()`: now receives `Channel<Msg>` instead of `ChannelId`
  - Old: `fn channel_open_session(&mut self, channel: ChannelId, session: &mut Session)`
  - New: `fn channel_open_session(&mut self, channel: Channel<Msg>, session: &mut Session) -> impl Future<Output = Result<bool, Self::Error>>`
  - Returns `bool` now (whether to accept)
- [ ] `pty_request()`: modes signature verified (still `&[(Pty, u32)]`)
- [ ] `signal()`: now takes `(channel: ChannelId, signal: Sig, session: &mut Session)`
- [ ] All handler methods now return `impl Future<Output = Result<(), Self::Error>>` — verify async fn compatibility

### 4.3 Response mechanism changes
- [ ] Replace ad-hoc response patterns with `Session::channel_success()` / `Session::channel_failure()`
  - `pty_request()` — call `session.channel_success(channel)` or `session.channel_failure(channel)`
  - `shell_request()` — same
  - `exec_request()` — same
  - `env_request()` — same
  - `window_change_request()` — same
  - `x11_request()` — same

### 4.4 File: `src/server/handler/mod.rs`
- [ ] `ServerHandler` — implement new trait methods with default behavior
- [ ] `auth_publickey()` — verify signature: `(&mut self, user: &str, public_key: &ssh_key::PublicKey)` unchanged
- [ ] `channel_open_session()` — update to accept `Channel<Msg>`, extract `ChannelId` via `.id()`
- [ ] `pty_request()` — should now reply with `session.channel_success(channel)`
- [ ] `signal()` — update signature

### 4.5 File: `src/server/handler/pty.rs`
- [ ] `forward_signal()` — verify `Sig` enum still matches

---

## Phase 5: Client Handler Trait

### 5.1 New required trait methods (all have defaults)
- [ ] `auth_banner()` — server auth banner
- [ ] `kex_done()` — key exchange completion callback
- [ ] `server_channel_open_forwarded_streamlocal()` — remote UDS forwarding
- [ ] `server_channel_open_agent_forward()` — agent forwarding
- [ ] `should_accept_unknown_server_channel()` — unknown channel type
- [ ] `server_channel_open_unknown()` — unknown channel handler
- [ ] `server_channel_open_session()` — server-initiated session
- [ ] `server_channel_open_direct_tcpip()` — server-initiated TCP/IP
- [ ] `server_channel_open_direct_streamlocal()` — server-initiated UDS
- [ ] `server_channel_open_x11()` — server-initiated X11
- [ ] `channel_success()` / `channel_failure()` — server responses
- [ ] `channel_open_failure()` — channel open rejection
- [ ] `xon_xoff()` — flow control notification
- [ ] `openssh_ext_host_keys_announced()` — host key announcement
- [ ] `disconnected()` — disconnect callback

### 5.2 Changed method signatures
- [ ] `server_channel_open_forwarded_tcpip()`: now receives `Channel<Msg>` (was old channel type)
- [ ] `check_server_key()`: takes `&ssh_key::PublicKey` — verify type compatibility
- [ ] `disconnected()`: now takes `DisconnectReason<Self::Error>` enum

### 5.3 File: `src/client/handler.rs`
- [ ] `ClientHandler` — implement new trait methods with default behavior
- [ ] `check_server_key()` — verify signature compatible
- [ ] `server_channel_open_forwarded_tcpip()` — verify `Channel<Msg>` usage
- [ ] `disconnected()` — update to new `DisconnectReason` enum

### 5.4 File: `src/client/mod.rs`
- [ ] `Session` struct — verify `russh::Channel<russh::client::Msg>` type still valid
- [ ] `ensure_channel()` — verify channel opening API unchanged
- [ ] `capture_exec()` — verify `ChannelMsg` enum variants still match

---

## Phase 6: Error Handling

### 6.1 New `russh::Error` variants
- [ ] `StrictKeyExchangeViolation { message_type, sequence_number }`
- [ ] `Signature(signature::Error)`
- [ ] `SshKey(ssh_key::Error)`
- [ ] `SshEncoding(ssh_encoding::Error)`
- [ ] `InvalidConfig(String)`
- [ ] `RecvError`
- [ ] `Pending`
- [ ] `DecryptionError`
- [ ] `RequestDenied`

### 6.2 Update error sites
- [ ] `src/error.rs:561` — `IroshError::Russh(#[from] russh::Error)` — verify `#[from]` still works (no duplicate `From` impls)
- [ ] `src/error.rs:288-352` — `ClientError` variants matching `russh::Error` — verify exhaustive
- [ ] `src/error.rs:463,501` — `russh::keys::ssh_key::Error` — verify path still resolves
- [ ] `src/client/mod.rs:133,162-163,187-188,356,372,391` — `russh::Error::ChannelOpenFailure` — remove overloaded sentinel usage
- [ ] `src/client/connect.rs:306,309,311,321,364` — error matching — update for new variants

### 6.3 ChannelOpenFailure sentinel cleanup
- [ ] `client/mod.rs:133` — replace `ChannelOpenFailure::ConnectFailed` sentinel with proper typed error
- [ ] `client/mod.rs:162-163` — same
- [ ] `client/mod.rs:187-188` — same
- [ ] `client/mod.rs:356` — same
- [ ] `client/mod.rs:372` — same
- [ ] `client/mod.rs:391` — same

---

## Phase 7: Key/Crypto API

### 7.1 `Ed25519Keypair::from_seed()` in `src/storage/keys.rs:133`
- [ ] Research: verify `ssh_key::private::Ed25519Keypair::from_seed()` still exists in ssh-key 0.7.0-rc.10
- [ ] If not: migrate to `ssh_key::private::KeypairData::Ed25519(Ed25519Keypair { keypair: ed25519_dalek::SigningKey::from_keypair_bytes(...), ... })`
- [ ] Test round-trip: `seed -> Ed25519Keypair -> PrivateKey -> authentication`

### 7.2 `PrivateKeyWithHashAlg` in `src/client/connect.rs:321`
- [ ] Verify `PrivateKeyWithHashAlg::new(Arc::new(client_key), None)` API unchanged
- [ ] Check `russh::keys::PrivateKeyWithHashAlg` re-exported in `keys/key.rs`

### 7.3 Import path verification
- [ ] `russh::keys::ssh_key::PublicKey` — verify still works via `pub use ssh_key;`
- [ ] `russh::keys::ssh_key::PrivateKey` — same
- [ ] `russh::keys::ssh_key::private::Ed25519Keypair` — same
- [ ] `russh::keys::ssh_key::HashAlg` — same
- [ ] `russh::keys::HashAlg` (direct) — also available via re-export
- [ ] `russh::keys::ssh_key::Error` — verify `#[from]` compat

---

## Phase 8: Channel API Changes

### 8.1 `Channel` split read/write halves
- [ ] Research: `Channel<Msg>` now has `.split()` → `(ChannelReadHalf, ChannelWriteHalf)`
- [ ] `src/client/handler.rs:167` — `channel: russh::Channel<russh::client::Msg>` — verify usage
- [ ] `src/server/handler/mod.rs:117` — `Channel<server::Msg>` — verify usage
- [ ] Replace `into_stream()` usage with split halves if beneficial

### 8.2 `streamed_channels` workaround cleanup
- [ ] `src/server/handler/mod.rs:22` — `streamed_channels: HashSet<ChannelId>` — evaluate if still needed
- [ ] `data()` handler — `streamed_channels` check — simplify if split halves handle it

### 8.3 `ChannelMsg` enum changes
- [ ] `src/client/mod.rs:554-575` — pattern match on `ChannelMsg` — add new variants
- [ ] New variants: `Open`, `OpenFailure`, `RequestPty`, `WindowChange`, `RequestX11`, `SetEnv`, `RequestShell`, `Exec`, `Signal`, `RequestSubsystem`, `AgentForward`, `Close`

---

## Phase 9: PTY and Signal Handling

### 9.1 PTY modes — `_modes` parameter
- [ ] `src/server/handler/mod.rs:166` — stop ignoring `modes`
- [ ] Research `portable-pty` API for applying terminal modes
- [ ] Forward `&[(Pty, u32)]` modes to the PTY session

### 9.2 `russh::Sig` verification
- [ ] `src/sys/unix/pty.rs:204-219` — `map_sig()` — verify all `Sig` variants unchanged
- [ ] `src/sys/unix/pty.rs:229-289` — tests — verify exhaustive
- [ ] `src/sys/windows/pty.rs:236` — `map_sig()` — verify `Sig` still has same variants
- [ ] `src/server/handler/pty.rs:682,708-709,735,743` — signal forwarding — verify `Sig` matches

---

## Phase 10: Client Channel Management

### 10.1 `Mutex<Option<russh::Channel<russh::client::Msg>>>`
- [ ] `src/client/mod.rs:80` — verify `Channel<client::Msg>` type still valid

### 10.2 `capture_exec` secondary channel
- [ ] `src/client/mod.rs:234-263` — verify channel creation API unchanged
- [ ] Ensure the secondary channel opened by `capture_exec` is properly tracked/closed

---

## Phase 11: Testing

### 11.1 Compile checks
- [ ] `cargo check --features server,client --workspace`
- [ ] `cargo check --all-features --workspace`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`

### 11.2 Unit tests
- [ ] `cargo test --lib --features client`
- [ ] `cargo test --lib --features server`
- [ ] `cargo test --lib --all-features`

### 11.3 Integration tests
- [ ] `cargo test --test '*' --all-features`
- [ ] `cargo test --workspace`

### 11.4 Manual verification
- [ ] SSH client → P2P → SSH server connection works
- [ ] Public key authentication works
- [ ] Password authentication works
- [ ] PTY allocation works (shell)
- [ ] Command execution works (exec)
- [ ] TCP port forwarding works
- [ ] Terminal resize propagation works
- [ ] Disconnect/cleanup works without hangs
- [ ] Re-key does not interrupt active sessions

---

## Phase 12: Polish

- [ ] `cargo fmt --all --check`
- [ ] Remove any `#[allow]` attributes no longer needed
- [ ] Update `temp/ref/russh/RESEARCH.md` with migration results
- [ ] Run full test suite one final time
- [ ] Commit with message: `upgrade: russh 0.55.0 → 0.61.1 — dependency conflict resolved, keepalive, strict kex, panic safety`
- [ ] Tag if appropriate

---

## Rollback Plan

If migration fails at any point:
1. `git checkout main -- Cargo.toml src/`
2. `cargo update -p russh`
3. Verify `ed25519-dalek` reversion in `Cargo.lock`
4. File issue with `temp/ref/russh/` findings
