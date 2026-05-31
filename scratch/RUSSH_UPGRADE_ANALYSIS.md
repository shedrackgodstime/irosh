# Russh 0.55 → 0.61.1 Upgrade Analysis

**Date**: 2026-05-31
**Status**: Dependency conflict resolved, ready for migration planning.

---

## 1. Dependency Conflict Resolved

The previous blocker — a diamond dependency conflict on `ed25519-dalek` — is now resolved:

| Dependency | Old (russh 0.60.3) | New (russh 0.61.1) | iroh 1.0.0-rc.0 |
|-----------|-------------------|-------------------|------------------|
| `ed25519-dalek` | 3.0.0-pre.6 (strict) | **3.0.0-pre.7** | 3.0.0-pre.7 |

Both russh and iroh now agree on `ed25519-dalek = 3.0.0-pre.7`. No `[patch]` or fork needed.

---

## 2. All Improvements from the Upgrade

### 2.1 Built-in SSH Keepalive (Closes a Gap)

**Current problem**: iroh has zero SSH-level keepalive. Connection health is only inferred from transport-level timeouts. A dropped connection (e.g., network partition, server crash) goes undetected until the next explicit I/O operation fails.

**Files affected** (7+ manual `tokio::time::timeout()` wrappers that could be simplified or replaced):
- `src/client/connect.rs:170,227,299,372,380` — wormhole discovery, P2P connect, SSH handshake, metadata timeouts
- `src/transport/iroh.rs:59,98` — endpoint online check
- `src/server/handler/pty.rs:572` — PTY cleanup timeout

**Russh 0.61 fix**: Config-level `keepalive_interval`, `keepalive_max`, `inactivity_timeout` on both `server::Config` and `client::Config`. Plus `Handle::send_keepalive()` and `Handle::send_ping()` methods for on-demand health checks.

### 2.2 Panic Safety Throughout the SSH Layer

**Current problem**: `src/client/connect.rs:249` has `.unwrap()` on `last_err` — panics if retry logic changes. The codebase also uses indexing and expects in SSH-related code.

**Russh 0.61 fix**: Crate-wide `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]`. All array accesses bounds-checked, all Results handled. The entire SSH layer becomes panic-free at the library level.

### 2.3 Simplified Channel Management

**Current problem**: The server handler maintains two separate tracking structures that can race:
1. `channels: HashMap<ChannelId, ChannelState>` — PTY state, env vars, process handles
2. `streamed_channels: HashSet<ChannelId>` — channels consumed by `into_stream()` for TCP forwarding

The `data()` handler must check both. If a channel ends up in both (not enforced), behavior is undefined.

On the client side, the primary channel is wrapped in `Mutex<Option<Channel>>` with double-check locking (`client/mod.rs:200-225`). `capture_exec` opens untracked secondary channels that can leak.

**Russh 0.61 fix**: `Channel` is split into `ChannelReadHalf` / `ChannelWriteHalf` / `ChannelStream`. The streamed channel workaround disappears — you read from one half, write to the other, no starvation. The new `Channel::split()` API eliminates the need for the `streamed_channels` tracking set entirely.

### 2.4 PTY Modes No Longer Discarded

**Current problem**: `src/server/handler/mod.rs:166` receives `_modes: &[(russh::Pty, u32)]` but discards it. SSH clients send important terminal settings (echo, onlcr, erase char, etc.) that are never applied. This breaks terminal behavior for clients depending on specific mode settings.

**Russh 0.61 fix**: The new handler signature makes PTY modes handling explicit. `Session::channel_success()` and `Session::channel_failure()` provide the proper response mechanism.

### 2.5 Strict Key Exchange (RFC 8308)

**Current problem**: No strict kex means vulnerability to MITM attacks that exploit sequential number weaknesses in the SSH protocol.

**Russh 0.61 fix**: Strict key exchange violation detection (`StrictKeyExchangeViolation` error) with proper kex sequencing. This is a real security improvement for P2P SSH connections.

### 2.6 Certificate Authentication Support

**Current problem**: SSH keys are derived from the Iroh endpoint secret seed (`storage/keys.rs:131-134`). The SSH identity and Iroh identity are permanently bound — cannot rotate SSH key independently. No support for CA-signed certificates.

**Russh 0.61 fix**: New `auth_openssh_certificate()` handler and `authenticate_openssh_cert()` client method. Enables optional migration to CA-signed certificates for node identity, with independent key rotation.

### 2.7 SSH Agent Forwarding

**Current problem**: No agent forwarding support. Users cannot use their local SSH agent through irosh tunnels.

**Russh 0.61 fix**: New `agent_request()` handler, `Handle::authenticate_publickey_with()` signer API, and full agent protocol support via `russh::keys::agent`.

### 2.8 Richer Error Types

**Current problem**: Error handling overloads `ChannelOpenFailure::ConnectFailed` as a "channel not available" sentinel in 5+ places (`client/mod.rs:131-392`). Error matching is fragile.

**Russh 0.61 fix**: New error variants enable precise error handling:
- `SshKey(ssh_key::Error)` — key parsing errors
- `SshEncoding(ssh_encoding::Error)` — encoding errors
- `RequestDenied` — server rejection
- `InvalidConfig(String)` — configuration errors
- `DecryptionError`, `Pending`, `RecvError` — protocol errors
- `StrictKeyExchangeViolation` — security errors
- `Signature(signature::Error)` — signature verification errors

### 2.9 Configurable Re-key

**Current problem**: No re-key at all. SSH sessions use the same keys indefinitely, increasing the window for cryptanalysis.

**Russh 0.61 fix**: `Limits { rekey_write_limit, rekey_read_limit, rekey_time_limit }` with sensible defaults (1 GB / 1 hour). Automatic re-key when limits are exceeded.

### 2.10 DH Group Exchange

**Current problem**: Default key exchange may use fixed DH groups. This limits security posture flexibility.

**Russh 0.61 fix**: Full `diffie-hellman-group-exchange-*` support with `Handler::lookup_dh_gex_group()` for custom group selection. Built-in safe prime database (`BUILTIN_SAFE_DH_GROUPS`).

### 2.11 Improved Config Structure

**Current problem**: `server::Config` and `client::Config` have minimal fields, forcing manual implementation of features like keepalive.

**Russh 0.61 fix**: Rich config with sensible defaults:
- `channel_buffer_size` — per-channel backpressure
- `event_buffer_size` — internal buffer sizing
- `inactivity_timeout` / `keepalive_interval` / `keepalive_max` — connection health
- `nodelay` — TCP_NODELAY control
- `auth_rejection_time_initial` — separate timing for initial none auth probe
- `client::Config.anonymous` — anonymous auth mode
- `client::Config.gex` — DH group exchange parameters

### 2.12 MSRV and Edition

- MSRV: 1.85 (matches iroh's `rust-version = "1.85"`)
- Edition: 2024 (matches iroh's `edition = "2024"`)
- No mismatch to worry about.

### 2.13 Crypto Backend Flexibility

**Current problem**: iroh pins `russh = { features = ["ring"] }`.

**Russh 0.61 fix**: `ring` still fully supported, but `aws-lc-rs` is now the default. Both backends are actively maintained. No change required to keep using `ring`.

---

## 3. Breaking Changes Inventory

| Area | Old (0.55) | New (0.61) | Migration Effort |
|------|-----------|-----------|-----------------|
| `Config` | Plain struct | `Arc<Config>` | Find all construction sites, wrap in Arc |
| `run_stream` | `(config, stream, handler)` | `(Arc<Config>, stream, handler)` | One call site |
| `connect_stream` | `(config, stream, handler)` | `(Arc<Config>, stream, handler)` | One call site |
| Server handler | Less methods | 25+ methods (all with defaults) | Add new trait methods with default impls |
| Client handler | Less methods | 20+ methods (all with defaults) | Add new trait methods with default impls |
| `channel_open_session` | `ChannelId` param | `Channel<Msg>` param | Change implementation signature |
| `Handle` | Opaque | `Handle<H: Handler>` generic | Type annotations may need updating |
| Channel API | Monolithic | Split read/write halves | Simplify `streamed_channels` workaround |
| `Ed25519Keypair::from_seed` | `from_seed(&[u8])` | May need `ssh_key::private::KeypairData` | Update key derivation in `storage/keys.rs` |
| Error matching | 7 variants | 20+ variants | Update error match arms |
| `ChannelMsg` | Data/ExitStatus/ExitSignal/Eof/Close | +Open/OpenFailure/RequestPty/WindowChange/etc | Update any exhaustive matches |
| Re-exports | `russh::keys::ssh_key::*` | `russh::keys::*` (re-exports `ssh_key`) | Verify import paths still work |
| `connect` | `(config, addr, handler)` | `(Arc<Config>, addr, handler)` | Update call sites |
| Crypto default | `ring` | `aws-lc-rs` | Keep `features = ["ring"]` to opt out |

---

## 4. Migration Phases

### Phase 1: Dependency Update
- Bump `russh` version in `Cargo.toml` from `0.55.0` to `0.61.1`
- Keep `features = ["ring"]` to retain existing crypto backend
- Run `cargo update` and verify resolution

### Phase 2: Config Migration
- Wrap all `server::Config` and `client::Config` in `Arc`
- Add new config fields (defaults are safe)
- Update `run_stream()` and `connect_stream()` call sites

### Phase 3: Handler Trait Migration
- Server handler: add new trait methods with default implementations
- Client handler: add new trait methods with default implementations
- Update `channel_open_session` signature to accept `Channel<Msg>`
- Update any method signatures that changed return types

### Phase 4: Error Handling
- Update error match arms for new variants
- Remove overloaded `ChannelOpenFailure::ConnectFailed` sentinel usage
- Handle new `SshKey`, `SshEncoding`, `StrictKeyExchangeViolation` errors

### Phase 5: Channel Simplification
- Migrate `into_stream()` usage to split read/write halves
- Remove `streamed_channels` tracking set
- Simplify `data()` handler logic

### Phase 6: Key Derivation
- Verify `Ed25519Keypair::from_seed()` API compatibility
- Test key round-trip: seed → keypair → PrivateKey → authentication

### Phase 7: Testing
- Run full test suite
- Test SSH client → server connection
- Test PTY allocation
- Test TCP forwarding
- Test keepalive timeout
- Test re-key behavior
