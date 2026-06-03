# Flow Audit: irosh v0.1.0 → v0.2.0 → v0.3.0 → v0.4.0

> Generated: comprehensive audit of user-facing workflow changes across all tagged releases.
> Purpose: identify every change that could break existing flows.

---

## Table of Contents

1. [File Existence Map](#1-file-existence-map)
2. [v0.1.0 → v0.2.0](#2-v010--v020)
   - 2.1 CLI Dispatch
   - 2.2 Auth Subsystem
   - 2.3 Identity Flow
   - 2.4 Trust Flow
   - 2.5 Wormhole Flow
   - 2.6 Peer Flow
   - 2.7 Connect Session Flow
   - 2.8 Host Server Flow
   - 2.9 System Service Flow
   - 2.10 IPC Flow
   - 2.11 Transfer Flow
   - 2.12 Config Passwd Check Dashboard
   - 2.13 Install Uninstall
3. [v0.2.0 → v0.3.0](#3-v020--v030)
   - 3.1–3.13 (same structure)
4. [v0.3.0 → v0.4.0](#4-v030--v040)
   - 4.1–4.13 (same structure)
5. [Cross-Cutting Risk Register](#5-cross-cutting-risk-register)
6. [Wire Protocol Compatibility](#6-wire-protocol-compatibility)

---

## 1. File Existence Map

A `—` means the file did not exist at that tag.

| File | v0.1.0 | v0.2.0 | v0.3.0 | v0.4.0 |
|------|--------|--------|--------|--------|
| *CLI layer* | | | | |
| `cli/bin/client.rs` | ✓ | — | — | — |
| `cli/bin/server.rs` | ✓ | — | — | — |
| `cli/src/main.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/mod.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/context.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/display.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/output.rs` | — | — | ✓ | ✓ |
| `cli/src/terminal.rs` | — | — | ✓ | ✓ |
| *CLI commands* | | | | |
| `cli/src/commands/connect/mod.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/connect/session.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/connect/tunnels.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/connect/input.rs` | — | — | ✓ | ✓ |
| `cli/src/commands/connect/prompt.rs` | — | — | ✓ | ✓ |
| `cli/src/commands/connect/transfer.rs` | — | — | — | ✓ |
| `cli/src/commands/connect/editor.rs` | — | — | ✓ | ✓ |
| `cli/src/commands/connect/history.rs` | — | — | ✓ | ✓ |
| `cli/src/commands/connect/completion.rs` | — | — | ✓ | ✓ |
| `cli/src/commands/host.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/system.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/dashboard.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/identity.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/trust.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/wormhole.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/peer.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/config.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/passwd.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/commands/check.rs` | — | ✓ | ✓ | ✓ |
| *CLI UI* | | | | |
| `cli/src/ui/mod.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/ui/feedback.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/ui/prompts.rs` | — | ✓ | ✓ | ✓ |
| `cli/src/ui/theme.rs` | — | ✓ | ✓ | ✓ |
| *Library* | | | | |
| `src/auth.rs` | — | ✓ | ✓ | ✓ |
| `src/config.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/diagnostic.rs` | — | ✓ | ✓ | ✓ |
| `src/error.rs` | ✓ | ✓ | ✓ | ✓ |
| *Client* | | | | |
| `src/client/mod.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/client/connect.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/client/handler.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/client/ipc.rs` | — | ✓ | ✓ | ✓ |
| `src/client/transfer/*` | ✓ | ✓ | ✓ | ✓ |
| *Server* | | | | |
| `src/server/mod.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/server/startup.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/server/handler/mod.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/server/handler/pty.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/server/shell_access.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/server/side_streams.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/server/ipc.rs` | — | ✓ | ✓ | ✓ |
| `src/server/transfer/files/blob.rs` | — | — | — | ✓ |
| *Session* | | | | |
| `src/session/mod.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/session/pty.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/session/state.rs` | ✓ | ✓ | ✓ | ✓ |
| *Storage* | | | | |
| `src/storage/keys.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/storage/trust.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/storage/peers.rs` | ✓ | ✓ | ✓ | ✓ |
| `src/storage/config.rs` | — | ✓ | ✓ | ✓ |
| `src/storage/shadow.rs` | — | ✓ | ✓ | ✓ |
| `src/storage/utils.rs` | — | ✓ | ✓ | ✓ |
| *Sys* | | | | |
| `src/sys/mod.rs` | — | ✓ | ✓ | ✓ |
| `src/sys/service.rs` | — | ✓ | ✓ | ✓ |
| `src/sys/signals.rs` | — | — | ✓ | ✓ |
| `src/sys/unix/*` | — | ✓ | ✓ | ✓ |
| `src/sys/windows/*` | — | ✓ | ✓ | ✓ |
| `src/sys/windows/job.rs` | — | — | — | ✓ |
| *Transport* | | | | |
| `src/transport/wormhole.rs` | — | ✓ | ✓ | ✓ |
| `src/transport/transfer/helpers.rs` | — | ✓ | ✓ | ✓ |
| *Scripts* | | | | |
| `public/scripts/install.sh` | ✓ | ✓ | ✓ | ✓ |
| `public/scripts/install.ps1` | ✓ | ✓ | ✓ | ✓ |
| `public/scripts/uninstall.sh` | ✓ | ✓ | ✓ | ✓ |
| `public/scripts/uninstall.ps1` | ✓ | ✓ | ✓ | ✓ |

---

## 2. v0.1.0 → v0.2.0

> **Scope**: 90+ commits. 139 files changed, +17,582 / -4,091 lines.
> **Nature**: Complete architecture rewrite from monolithic binaries to library + unified CLI.

---

### 2.1 CLI Dispatch

| # | Change | Location | Risk |
|---|--------|----------|------|
| C1 | **3 binaries → 1 binary**: v0.1.0 had `irosh`, `irosh-server`, `irosh-client`. v0.2.0 has only `irosh`. | `cli/Cargo.toml`, `cli/bin/*` → `cli/src/main.rs` | **HIGH** — scripts using `irosh-server` or `irosh-client` will fail |
| C2 | **Command model replaced**: v0.1.0 inline commands (`List`, `Save`, `Delete`, `Trust`) → v0.2.0 `Commands` enum (`Connect`, `Host`, `Wormhole`, `System`, `Peer`, `Trust`, `Passwd`, `Identity`, `Config`, `Check`) | `cli/src/commands/mod.rs` (new) | **HIGH** — `irosh list`, `irosh save`, `irosh delete` no longer work |
| C3 | **Async main**: `fn main()` → `#[tokio::main] async fn main()` | `cli/src/main.rs` | **HIGH** — all handlers must be async |
| C4 | **`--state` flag**: short form `-s` removed; env var `IROSH_STATE_DIR` → `IROSH_STATE` | `cli/src/main.rs` | **MEDIUM** — scripts using `-s` or old env var break |
| C5 | **Error handling**: `return Err(...)` → `Ui::error()` + `std::process::exit(1)` | `cli/src/main.rs` | **LOW** — output format changes |
| C6 | **Logging**: monochrome, `without_time()` when not verbose, Windows VT processing | `cli/src/main.rs` | **LOW** |

### 2.2 Auth Subsystem

| # | Change | Location | Risk |
|---|--------|----------|------|
| A1 | **ServerHandler::new signature**: `(Vec<PublicKey>, SecurityConfig, StateConfig, ConnectionShellState)` → `(Arc<dyn Authenticator>, ConnectionShellState)` | `src/server/handler/mod.rs:10-30` | **BREAKING** — all server construction code must be rewritten |
| A2 | **Auth trait introduced**: pluggable `Authenticator` trait replaces inline key check | `src/auth.rs` (new) | **MEDIUM** — old `SecurityConfig` no longer used |
| A3 | **Password auth added**: server now supports `auth_password` (v0.1.0 was key-only) | `src/server/handler/mod.rs` | **LOW** — new capability |
| A4 | **AuthError enum**: 4 new error variants | `src/error.rs` | **LOW** — additive, but `matches!` on `IroshError` must be non-exhaustive |
| A5 | **Error text changes**: "SSH error" → "ssh protocol error", "SSH authentication failed" → "authentication failed" | `src/error.rs` | **LOW** — breaks string matching |

### 2.3 Identity Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| I1 | **Dedicated identity command**: monolithic `print_identity()` → `IdentityAction::Show` / `IdentityAction::Rotate` | `cli/src/commands/identity.rs` (new) | **LOW** — new feature |
| I2 | **load_secret_key() added**: reads key without generating | `src/storage/keys.rs:73-98` | **LOW** — new API |
| I3 | **NodeSecretInvalid gains `source` field**: `{ details }` → `{ details, source }` | `src/storage/keys.rs:104-106` | **MEDIUM** — destructuring with 2 fields breaks |
| I4 | **rotate_identity() added**: delete + regenerate | `src/storage/mod.rs:46-53` | **LOW** |

### 2.4 Trust Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| T1 | **Dedicated trust command**: new `List`, `Revoke`, `Reset` actions | `cli/src/commands/trust.rs` (new) | **LOW** |
| T2 | **Directory structure**: `trust/servers/`, `trust/clients/` directories created | `src/storage/trust.rs:68-70` | **LOW** |

### 2.5 Wormhole Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| W1 | **Wormhole transport module**: new rendezvous protocol | `src/transport/wormhole.rs` (new) | **LOW** — new feature |
| W2 | **Shadow password storage**: `load_shadow_file`, `write_shadow_file`, `delete_shadow_file` | `src/storage/shadow.rs` (new) | **LOW** |
| W3 | **Atomic write utilities**: `atomic_write_secure`, `ensure_dir_secure` | `src/storage/utils.rs` (new) | **LOW** |
| W4 | **Wormhole CLI**: `Open { code, passwd, persistent }`, `Status`, `Disable` | `cli/src/commands/wormhole.rs` (new) | **LOW** |

### 2.6 Peer Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| P1 | **Dedicated peer command**: `List`, `Add { name, ticket }`, `Remove { name }`, `Info { name }` | `cli/src/commands/peer.rs` (new) | **LOW** |
| P2 | **`save_peer`**: `serde_json::to_string_pretty` + `fs::write` → `to_vec_pretty` + `atomic_write_secure` | `src/storage/peers.rs:60-68` | **MEDIUM** — atomic write may fail on permission-restricted FS |
| P3 | **DirectoryEntryRead gains `path` field** | `src/storage/peers.rs:108-112` | **MEDIUM** — error pattern matching breaks |

### 2.7 Connect / Session Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| S1 | **`Client::connect` delegated**: `dial_p2p` + `establish_session` instead of inline | `src/client/connect.rs` | **MEDIUM** |
| S2 | **ClientOptions added**: `relay_mode`, `credentials`, `prompter: Arc<dyn PasswordPrompter>` | `src/client/connect.rs` | **LOW** |
| S3 | **ResolvedTarget enum**: `Ticket` and `WormholeCode` variants | `src/client/connect.rs` | **LOW** |
| S4 | **SSH handshake timeout**: 60-second timeout added | `src/client/connect.rs` | **MEDIUM** — connections that hung forever now fail fast |
| S5 | **`SESSION_USER`**: `"demo"` → `"irosh"` | `src/client/connect.rs` | **HIGH** — remote shell sees different username |
| S6 | **`METADATA_OPEN_TIMEOUT`**: 2s → 5s | `src/client/connect.rs` | **LOW** |
| S7 | **`handle` wrapped in `Arc<RwLock<...>>`**: `channel` from owned → `Option` | `src/client/mod.rs` | **HIGH** — all callers need lock acquisition; `&mut self` methods face contention |
| S8 | **SessionEvent variants renamed**: `Stdout(Vec<u8>)` → `Data(Vec<u8>)`, `Stderr(Vec<u8>)` → `ExtendedData(Vec<u8>, u32)`; added `ExitSignal`, `Ignore` | `src/client/mod.rs` | **BREAKING** — old variant names gone |
| S9 | **`next_event` no longer loops**: returns `Ok(None)` when channel is `None` | `src/client/mod.rs` | **MEDIUM** — callers must handle `None` |
| S10 | **Writer changed**: `Box<dyn Write + Send>` → `UnboundedSender<Vec<u8>>` + `CancellationToken` | `src/server/handler/pty.rs` | **HIGH** — writer API completely different |
| S11 | **Unix PTY reader**: `spawn_blocking` → `AsyncFd` with `tokio::select!` | `src/server/handler/pty.rs` | **MEDIUM** — cancellation behavior changed |
| S12 | **Disconnect sequence**: now closes channel → handle → connection → endpoint (previously handle + endpoint only) | `src/client/connect.rs` | **MEDIUM** — more thorough but potential double-close |
| S13 | **`parse_target`**: returns `ResolvedTarget` instead of `Ticket` | `src/client/connect.rs` | **BREAKING** |
| S14 | **Password auth fallback**: if public key rejected, try password via credentials/prompter | `src/client/connect.rs` | **LOW** — new behavior |
| S15 | **Port forwarding**: `channel_open_direct_tcpip` handler added | `src/client/handler.rs` | **LOW** |
| S16 | **Connect module**: new with wormhole code support, peer selection | `cli/src/commands/connect/mod.rs` | **LOW** |
| S17 | **Session drive loop**: stdin → send, event → stdout/stderr, resize forwarding | `cli/src/commands/connect/session.rs` | **LOW** |

### 2.8 Host / Server Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| H1 | **Server::inspect**: takes `&ServerOptions` (was consuming) | `src/lib.rs` | **MEDIUM** — owned-value callers break |
| H2 | **Server::bind returns tuple**: `(ServerReady, Server)` with IPC-enabled `Server` | `src/lib.rs` | **MEDIUM** |
| H3 | **Server fields added**: `ipc_enabled`, `shutdown_tx/rx`, `control_tx/rx`, `ticket`, `gossip`, `wormhole_confirmation`, `shutdown_on_wormhole_success`; `authenticator` replaces `authorized_clients` | `src/server/mod.rs:166` | **HIGH** — direct `Server` construction breaks |
| H4 | **ServerOptions fields added**: `ipc_enabled`, `relay_mode`, `relay_url`, `authenticator`, `wormhole_confirmation`, `shutdown_on_wormhole_success`; `authorized_key()` removed | `src/server/mod.rs` | **MEDIUM** |
| H5 | **ServerShutdown::close**: `&self` → consuming `self` | `src/server/mod.rs` | **BREAKING** |
| H6 | **bind_server**: single ALPN → ALPN list + relay mode; creates gossip, channels, `UnifiedAuthenticator` | `src/server/startup.rs` | **MEDIUM** |
| H7 | **Host command**: `--simple`, `--port`, `--json` flags | `cli/src/commands/host.rs` (new) | **LOW** |

### 2.9 System / Service Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| SY1 | **sys module introduced**: platform-agnostic service abstractions | `src/sys/` (new) | **LOW** |
| SY2 | **Service unit name**: Linux: `irosh-server.service` → `irosh.service` | `src/sys/unix/service.rs` | **HIGH** — upgrade won't auto-migrate; service appears "not installed" |
| SY3 | **Windows service**: Task Scheduler → SCM | `src/sys/windows/service.rs` | **HIGH** — old scheduled task not cleaned up |
| SY4 | **System command**: Install, Uninstall, Start, Stop, Restart, Status, Logs | `cli/src/commands/system.rs` (new) | **LOW** |

### 2.10 IPC Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| IP1 | **IPC introduced**: `IpcCommand`/`IpcResponse` over Unix Domain Socket / Windows Named Pipe | `src/server/ipc.rs` (new) | **LOW** |
| IP2 | **IpcClient**: connects to socket/pipe, JSON round-trip | `src/client/ipc.rs` (new) | **LOW** |
| IP3 | **IPC server**: reads up to 64KB JSON, dispatches, writes response | `src/server/ipc.rs` (new) | **LOW** |

### 2.11 Transfer Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| TR1 | **LiveShellContext → ShellContext (enum)**: singleton struct → two-variant enum | `src/server/transfer/state.rs` | **BREAKING** — pattern matches break |
| TR2 | **spawn_upload_helper**: returns `UploadSink` enum instead of `Child` | `src/server/transfer/helpers.rs` | **BREAKING** — callers awaiting child directly break |
| TR3 | **spawn_download_helper**: returns `DownloadSource` enum instead of `Child` | `src/server/transfer/helpers.rs` | **BREAKING** |
| TR4 | **`resolve_remote_path` → `ShellContext::resolve_path`**: sync → async | `src/server/transfer/state.rs` | **BREAKING** |
| TR5 | **TransferFrame kinds added**: kinds 14-17 for completion/entry tracking | `src/transport/transfer/types.rs` | **LOW** — forward compat; old peers reject unsupported kinds |
| TR6 | **recursive field added** to PutRequest/GetRequest with `#[serde(default)]` | `src/transport/transfer/types.rs` | **LOW** — backward-compatible deserialization |
| TR7 | **server_home_dir removed**: replaced with `ShellContext::home_dir()` | `src/server/transfer/state.rs` | **BREAKING** |

### 2.12 Config / Passwd / Check / Dashboard

| # | Change | Location | Risk |
|---|--------|----------|------|
| CF1 | **AppConfig struct**: `stealth_secret`, `relay_url`, `log_level`, `wormhole_timeout`, `default_user` | `src/config.rs` | **LOW** |
| CF2 | **Storage config**: load/save for AppConfig | `src/storage/config.rs` (new) | **LOW** |
| CF3 | **Config, Passwd, Check, Dashboard commands**: all new | `cli/src/commands/` (new) | **LOW** |

### 2.13 Install / Uninstall

| # | Change | Location | Risk |
|---|--------|----------|------|
| IN1 | **3 binaries → 1 binary installed**: `irosh` only | `install.sh:93-103`, `install.ps1:64-69` | **HIGH** — upgrade leaves stale `irosh-server`/`irosh-client` |
| IN2 | **Service install command**: `irosh-server service install` → `irosh system install` | `install.sh:110`, `install.ps1:108` | **HIGH** — old command not found |
| IN3 | **Post-install identity preview**: new step | `install.sh:117-121` | **LOW** |
| IN4 | **Guidance text**: `irosh-server --simple` → `irosh host` | `install.sh:126-134` | **HIGH** — v0.1.0 script references non-existent binary |
| IN5 | **Uninstall.sh**: legacy binary cleanup added (`irosh-server`, `irosh-client`) | `uninstall.sh:53-64` | **LOW** |
| IN6 | **Uninstall.ps1**: service stop + uninstall via `schtasks /delete` | `uninstall.ps1:45-56` | **LOW** |
| IN7 | **Uninstall.ps1**: firewall rule cleanup | `uninstall.ps1:59-62` | **LOW** |
| IN8 | **Windows firewall rule added**: `New-NetFirewallRule` for P2P UDP | `install.ps1:95-103` | **LOW** |

---

## 3. v0.2.0 → v0.3.0

> **Scope**: 86 commits. 86 files changed, +7,021 / -1,507 lines.
> **Nature**: Terminal stabilization, input engine rewrite, Windows SCM service, IPC transport change.

---

### 3.1 CLI Dispatch

| # | Change | Location | Risk |
|---|--------|----------|------|
| C7 | **Global `--json` and `--yes` (`-y`)** flags added | `cli/src/main.rs` | **LOW** — new optional flags |
| C8 | **Windows SCM entry**: `run_service()` called before `Args::parse()` | `cli/src/main.rs` | **LOW** — only affects SCM launch path |
| C9 | **Connect subcommand**: new `--secret`, `-s` flag | `cli/src/commands/mod.rs:35-36` | **LOW** |
| C10 | **Status alias**: `check` and `status` both route to `check::exec` | `cli/src/commands/mod.rs:120` | **LOW** |
| C11 | **Peer Add/Info fields**: `String` → `Option<String>` for interactive prompts | `cli/src/commands/mod.rs:148-154` | **MEDIUM** — clap accepts missing positionals but behavior changes |
| C12 | **IpcClient removed from CliContext**: created on-demand now | `cli/src/context.rs` | **LOW** |

### 3.2 Auth Subsystem

| # | Change | Location | Risk |
|---|--------|----------|------|
| A6 | **AuthMode enum added**: `Key`, `Password`, `Combined`, `Unified` | `src/auth.rs:44-54` | **LOW** |
| A7 | **AuthMethod gains Serialize/Deserialize** | `src/auth.rs:36` | **LOW** |
| A8 | **`_temp_password_hash` no longer ignored**: now used for wormhole password comparison | `src/auth.rs:396-436` | **BEHAVIOR** — code that passed temp hash before now actively uses it |
| A9 | **`check_password_match` return**: `Result<bool>` → `Result<Option<bool>>` | `src/auth.rs:457-475` | **MEDIUM** — callers must handle `None` |
| A10 | **Rate limiting**: `check_public_key` returns `Ok(false)` if ≥3 failed attempts | `src/auth.rs:487-490` | **LOW** — security improvement |

### 3.3 Identity Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| I5 | **Key file write**: `fs::write` → `atomic_write_secure` | `src/storage/keys.rs:124-130` | **MEDIUM** — atomic write may fail on permission-restricted FS |
| I6 | **save_secret_key**: `fs::write` → `atomic_write_secure` | `src/storage/keys.rs:167-168` | **MEDIUM** — same |

### 3.4 Trust Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| T3 | **safe_id strips `:`**: added to sanitized characters | `src/storage/trust.rs:77,87` | **MEDIUM** — path mismatch for old trust files with `:` in ID |
| T4 | **write_public_key**: `key.write_openssh_file(path)` → `key.to_openssh()` + `atomic_write_secure()` | `src/storage/trust.rs:93-98` | **MEDIUM** — temp+rename may fail |
| T5 | **load_all_authorized_clients return**: `Vec<PublicKey>` → `Vec<(String, PublicKey)>` | `src/storage/trust.rs:177-178` | **BREAKING** — all callers must update destructuring |
| T6 | **DirectoryEntryRead gains path field** | `src/storage/trust.rs:194-200` | **BREAKING** — error matching breaks |
| T7 | **Legacy files tagged**: old `authorized_client.pub` → `("legacy", key)` | `src/storage/trust.rs:211-213` | **LOW** |
| T8 | **`fs::create_dir_all` → `ensure_dir_secure`**: 0700/0600 permissions | `src/storage/trust.rs:54-58` | **MEDIUM** — mask may be too restrictive |

### 3.5 Wormhole Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| W5 | **Daemon readiness retry**: up to 6× 500ms retries | `cli/src/commands/wormhole.rs:22-44` | **LOW** |
| W6 | **Wormhole blocked when vault populated + no password**: won't open wormhole | `cli/src/commands/wormhole.rs:69-78` | **MEDIUM** — unpassworded servers with trusted devices can't wormhole |
| W7 | **Code length enforcement**: custom codes ≥8 chars if no session password | `cli/src/commands/wormhole.rs:83-99` | **MEDIUM** — short-code scripts break |
| W8 | **check_password_match semantics**: `Some(true)` = wormhole, `Some(false)` = node pw, `None` = no match | `src/auth.rs:567-593` | **MEDIUM** — password behavior changed |

### 3.6 Peer Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| P4 | **`get_peer` → `load_peer`**: function renamed | `src/storage/peers.rs:76` | **BREAKING** |
| P5 | **`rename_peer` added** | `src/storage/mod.rs:22` | **LOW** |
| P6 | **Ticket validation on add**: early parse prevents saving invalid tickets | `cli/src/commands/peer.rs:74-75` | **MEDIUM** — scripts that passed invalid tickets fail earlier |
| P7 | **Duplicate name detection**: rejects re-add with existing name | `cli/src/commands/peer.rs:103-115` | **MEDIUM** — scripts replacing peers by re-adding break |
| P8 | **Auto-save**: interactive confirm → silent with conflict resolution | `cli/src/commands/connect/mod.rs:218-253` | **MEDIUM** — interactive flow changed |

### 3.7 Connect / Session Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| S18 | **Dial retry + timeout**: 20s CONNECT_TIMEOUT, 3 retries, 500ms delay | `src/client/connect.rs` | **MEDIUM** — connections that hung now fail fast |
| S19 | **parse_target handles `{` prefix**: ticket heuristic | `src/client/connect.rs` | **LOW** |
| S20 | **`#[must_use]` on ClientOptions** | `src/client/connect.rs` | **LOW** |
| S21 | **last_err**: `expect()` → safe `unwrap_or()` — **fixes potential panic** | `src/client/connect.rs` | **LOW** — bugfix |
| S22 | **Input Engine introduced** (`input.rs`): full state machine for keystrokes, escape sequences (`~.`, `~C`, `~?`, `~~`), ANSI tracking, editor, history, tab completion | `cli/src/commands/connect/input.rs` (new) | **HIGH** — raw pass-through mode replaced with smart input processing |
| S23 | **Raw terminal**: `libc`-based `RawTerminal` → crossterm-based `TerminalGuard` | `cli/src/terminal.rs` (new) | **MEDIUM** — different terminal flags, edge cases on non-Linux |
| S24 | **Session drive rewrite**: takes `InputEngine` + `TransferContext`; local commands, escape handling, remote data buffering during local edit | `cli/src/commands/connect/session.rs` | **HIGH** — completely different session loop |
| S25 | **CPR query**: Windows DSR `\x1b[6n` for cursor position | `cli/src/commands/connect/session.rs` | **LOW** |
| S26 | **Local commands**: `help`, `lpwd`, `lls`, `lcd`, `paths`, `exit`, `disconnect`, `clear`, `put`, `get` | `cli/src/commands/connect/prompt.rs` (new) | **LOW** |
| S27 | **Transfer CLI**: `TransferContext` with progress bars, Ctrl+C cancellation | `cli/src/commands/connect/transfer.rs` (new) | **LOW** |
| S28 | **Tab completion**: keywords, local paths, remote paths | `cli/src/commands/connect/completion.rs` (new) | **LOW** |
| S29 | **Line editor**: insert, backspace, delete, arrows, home/end, history, tab, submit | `cli/src/commands/connect/editor.rs` (new) | **LOW** |
| S30 | **Command history**: persistent to filesystem (`escape.history`, `prompt.history`) | `cli/src/commands/connect/history.rs` (new) | **LOW** |
| S31 | **Windows PTY reader optimization**: mpsc channel → direct `block_on` (eliminates intermediate queue) | `src/server/handler/pty.rs` | **MEDIUM** — blocking-coop risk under load |
| S32 | **Child-exit race logic**: now waits 500ms for reader to finish after child exits | `src/server/handler/pty.rs` | **LOW** — safety margin |

### 3.8 Host / Server Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| H8 | **Server::run uses JoinSet**: sessions tracked in `JoinSet` instead of simple loop | `src/server/mod.rs:220` | **MEDIUM** |
| H9 | **Wormhole rate limiting**: `ActiveWormhole` struct with `failed_attempts: Arc<AtomicU32>` | `src/server/mod.rs` | **LOW** |
| H10 | **ServerOptions**: `auth_mode`, `auth_mode` added; `wormhole_confirmation` removed | `src/server/mod.rs` | **MEDIUM** |
| H11 | **Side streams protocol break**: metadata-first → magic-byte dispatch (`IRFT` at start of every stream) | `src/server/side_streams.rs` | **BREAKING** — old clients connecting to v0.3.0+ servers will fail metadata exchange |
| H12 | **spawn_metadata_and_transfer_acceptor → spawn_side_stream_listener**: function renamed | `src/server/side_streams.rs` | **BREAKING** |
| H13 | **Host command**: `--auth-mode`, `--authorize`, `--simple` flags added; signal handling via `wait_for_shutdown_signal()` | `cli/src/commands/host.rs` | **LOW** |
| H14 | **IPC daemon conflict detection**: checks for existing socket file | `cli/src/commands/host.rs` | **LOW** |

### 3.9 System / Service Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| SY5 | **Status probes daemon via IPC**: `IpcClient::send(GetStatus)` for live info | `cli/src/commands/system.rs` | **LOW** |
| SY6 | **`--json` output**: `SystemStatusResponse` struct | `cli/src/commands/system.rs` | **LOW** |
| SY7 | **view_logs signature**: `view_logs(follow)` → `view_logs(follow, state: Option<PathBuf>)` | `cli/src/commands/system.rs`, `src/sys/service.rs` | **BREAKING** — signature change |
| SY8 | **query_service_status signature**: no arg → `query_service_status(state: Option<PathBuf>)` | `cli/src/commands/system.rs`, `src/sys/service.rs` | **BREAKING** |

### 3.10 IPC Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| IP4 | **Windows IPC: Named Pipes → TCP loopback**: `TcpListener::bind("127.0.0.1:0")`, port written to `ipc.port` file | `src/server/ipc.rs` | **BREAKING** — old client can't communicate with new daemon and vice versa |
| IP5 | **Shutdown signal**: IPC server now takes `shutdown_rx`; uses `tokio::select!` | `src/server/ipc.rs` | **LOW** |
| IP6 | **Socket cleanup**: on Unix, `remove_file` on exit; on Windows, delete `ipc.port` | `src/server/ipc.rs` | **LOW** |
| IP7 | **DaemonStatus struct**: `endpoint_id`, `ticket`, `wormhole_active`, `wormhole_code`, `active_sessions` | `src/server/ipc.rs` | **LOW** |
| IP8 | **IpcResponse::Status**: struct-variant with named fields → `Status(DaemonStatus)` tuple-variant | `src/server/ipc.rs` | **BREAKING** — pattern matching on old struct breaks |
| IP9 | **Client: Windows NamedPipeClient → TcpStream**: reads port from `ipc.port` | `src/client/ipc.rs` | **BREAKING** |
| IP10 | **`stream.shutdown()` no longer `#[cfg(unix)]`-gated**: called unconditionally | `src/client/ipc.rs` | **MEDIUM** — Windows TcpStream supports shutdown |

### 3.11 Transfer Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| TR8 | **`handle_transfer_stream` signature**: `(send, recv)` → `(stream: IrohDuplex)` | `src/server/transfer/mod.rs` | **BREAKING** |
| TR9 | **All handlers take `&shell_state: &ConnectionShellState`** | `src/server/transfer/mod.rs` | **BREAKING** |
| TR10 | **ServerError::TransferFailed**: `{ details: String }` → `{ failure: TransferFailure }` | `src/error.rs` | **BREAKING** |
| TR11 | **ClientError::TransferFailed**: `{ details: String }` → `{ failure: TransferFailure }` | `src/error.rs` | **BREAKING** |
| TR12 | **ClientError::TransferRejected**: variant removed, merged into `TransferFailed` | `src/error.rs` | **BREAKING** |
| TR13 | **TransferFailureCode**: added `NotFound`, `IsDirectory` variants | `src/transport/transfer/types.rs` | **BREAKING** — exhaustive match fails |
| TR14 | **ConnectionShellState::new()**: takes `PathBuf` | `src/server/transfer/state.rs` | **BREAKING** |
| TR15 | **ShellContext::cwd**: 2s TTL cache | `src/server/transfer/state.rs` | **BEHAVIOR** — cached CWD may be stale |
| TR16 | **`server_home_dir()` removed**: replaced by `ShellContext::home_dir()` | `src/server/transfer/state.rs` | **BREAKING** |
| TR17 | **ShellContext::remove_file**: returns `Result<()>` instead of `()` | `src/server/transfer/state.rs` | **BREAKING** |
| TR18 | **ShellContext::rename**: no longer uses `-f` flag | `src/server/transfer/state.rs` | **BEHAVIOR** — may fail if target exists |

### 3.12 Config / Passwd / Check / Dashboard

| # | Change | Location | Risk |
|---|--------|----------|------|
| CF4 | **Config**: `--json` output added for List and Get | `cli/src/commands/config.rs` | **LOW** |
| CF5 | **Passwd**: `--json` mode removes confirmation for Remove; `Status` uses Ui::header | `cli/src/commands/passwd.rs` | **LOW** |
| CF6 | **Check**: major rewrite — uses `Ui::header`/`success`/`warn`/`error`/`status` instead of raw `println!`; `--json` output | `cli/src/commands/check.rs` | **LOW** |
| CF7 | **Dashboard**: `IpcResponse::Status` destructured differently (struct → struct field) | `cli/src/commands/dashboard.rs` | **MEDIUM** — must match new variant shape |

### 3.13 Install / Uninstall

| # | Change | Location | Risk |
|---|--------|----------|------|
| IN9 | No changes to install scripts between v0.2.0 and v0.3.0 | | |
| IN10 | No changes to uninstall scripts | | |

---

## 4. v0.3.0 → v0.4.0

> **Scope**: 11 commits. 109 files changed, +4,899 / -8,057 lines.
> **Nature**: iroh 1.0 migration, blob-based transfers, --exec feature, Router-based server architecture.

---

### 4.1 CLI Dispatch

| # | Change | Location | Risk |
|---|--------|----------|------|
| C13 | **`--exec` (`-e`) flag**: run single remote command and exit | `cli/src/commands/mod.rs:37` | **LOW** |
| C14 | **`classify_error()` function**: context-specific error tips | `cli/src/main.rs` | **LOW** |
| C15 | **Iroh 1.0-rc.0 dependency**: `iroh` 0.96.0 → 1.0.0-rc.0, `iroh-tickets` 0.3.0 → 1.0.0-rc.0, `iroh-blobs` added | `Cargo.toml` | **MEDIUM** — API changes from pre-1.0 to rc could cause compilation failures |

### 4.2 Auth Subsystem

| # | Change | Location | Risk |
|---|--------|----------|------|
| A11 | **`Credentials.password`**: `String` → `SecretString` (zeroizing type) | `src/auth.rs` | **BREAKING** — must use `.expose_secret()` |
| A12 | **`record_failure()`**: shared failure counting logic extracted | `src/auth.rs:456-467` | **LOW** — refactor |
| A13 | **`check_public_key`**: calls `record_failure()` on reject | `src/auth.rs:569,597` | **LOW** — consistent rate limiting |

### 4.3 Identity Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| I7 | **`NodeIdentity` → `EndpointIdentity`**: struct renamed | `src/storage/keys.rs:32` | **BREAKING** |
| I8 | **`node_id()` → `endpoint_id()`**: method renamed | `src/storage/keys.rs:42-46` | **BREAKING** |
| I9 | **`SECRET_KEY_FILE`**: `"keys/node.secret"` → `"keys/endpoint.secret"` | `src/storage/keys.rs:52` | **CRITICAL** — existing users get a new identity on upgrade! |
| I10 | **`SecretKey::generate(&mut rand::rng())` → `SecretKey::generate()`**: implicit RNG | `src/storage/keys.rs:116-117` | **LOW** |
| I11 | **`NodeSecretInvalid` → `EndpointSecretInvalid`**: error variant renamed | `src/storage/keys.rs:99,112` | **BREAKING** |
| I12 | **JSON output**: `node_id` → `endpoint_id` in JSON field | `cli/src/commands/identity.rs:32-38` | **MEDIUM** — scripts parsing JSON break |
| I13 | **UI text**: "Node ID" → "Endpoint ID" | `cli/src/commands/identity.rs:48-60` | **LOW** |

### 4.4 Trust Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| T9 | Unit tests added only | `src/storage/trust.rs:366-512` | **LOW** |

### 4.5 Wormhole Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| W9 | **Error messages**: `Ui::error(msg)` → `Ui::error(msg, hint)` | `cli/src/commands/wormhole.rs:80-83` | **MEDIUM** — script parsing of error output breaks |

### 4.6 Peer Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| P9 | **JSON field**: `node_id` → `endpoint_id` | `cli/src/commands/peer.rs:139,218` | **MEDIUM** — script parsing of JSON output breaks |
| P10 | **Error messages**: reformatted with hints | `cli/src/commands/peer.rs` | **MEDIUM** — script parsing of error output breaks |

### 4.7 Connect / Session Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| S33 | **`dial_p2p` returns 3-tuple**: `(Connection, Endpoint, FsStore)` instead of `(Connection, Endpoint)` | `src/client/connect.rs` | **BREAKING** |
| S34 | **Blobs ALPN**: `vec![alpn]` → `vec![alpn, blobs_alpn]` | `src/client/connect.rs` | **MEDIUM** — peer may not negotiate unknown ALPN |
| S35 | **BlobsProtocol spawned**: via `Router::builder(...).accept(...).spawn()` | `src/client/connect.rs` | **LOW** |
| S36 | **channel**: `Option<Channel>` → `tokio::sync::Mutex<Option<Channel>>` | `src/client/mod.rs` | **MEDIUM** — mutex introduces potential deadlock |
| S37 | **`&mut self` → `&self`**: `request_pty`, `start_shell`, `exec`, `ensure_channel`, `send`, `eof`, `resize`, `next_event`, `disconnect` all changed | `src/client/mod.rs` | **MEDIUM** — callers no longer need `&mut` but must handle mutex |
| S38 | **`ensure_channel`**: double-checked locking pattern | `src/client/mod.rs` | **MEDIUM** — race eliminated but complexity added |
| S39 | **Non-interactive exec mode**: `--exec` bypasses input engine | `cli/src/commands/connect/mod.rs` | **LOW** |
| S40 | **Connection_info**: destructured from 2-tuple to 3-tuple | `cli/src/commands/connect/mod.rs` | **MEDIUM** — fails if destructured as 2-tuple |
| S41 | **Upload**: `upload_with_progress()` → `upload_blob()` (blob-based) | `cli/src/commands/connect/transfer.rs` | **MEDIUM** — new API, different return type |
| S42 | **Download**: `download_with_progress()` → `download_blob()` (blob-based) | `cli/src/commands/connect/transfer.rs` | **MEDIUM** — new API |
| S43 | **BLAKE3 hash printed on upload completion** | `cli/src/commands/connect/transfer.rs` | **LOW** |
| S44 | **Transfer error downcasting**: `if let IroshError::Client(client_err) = &err` → `if let Some(IroshError::Client(client_err)) = err.downcast_ref::<IroshError>()` | `cli/src/commands/connect/transfer.rs` | **LOW** — internal |
| S45 | **Windows Ctrl+Break injection**: `\x1c` byte added for QUIT/ABRT signals | `src/server/handler/pty.rs` | **LOW** |
| S46 | **Password prompter error**: `unwrap_or_else` logging instead of `.ok().flatten()` | `src/client/connect.rs` | **LOW** — internal |
| S47 | **`ConnectionShellState::new()`**: takes `blobs` argument | `src/server/transfer/state.rs`, `src/client/tests/auth.rs` | **BREAKING** |

### 4.8 Host / Server Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| H15 | **`Server::run` architecture**: `accept()` loop → `Router` with `ProtocolHandler` traits (`SshProtocol`, `GossipProtocol`) | `src/server/mod.rs` | **BREAKING** — internal dispatch completely restructured |
| H16 | **`ActiveWormhole`**: moved from inside `run()` to module level | `src/server/mod.rs` | **LOW** — refactor |
| H17 | **Session tracking**: `SessionTracker` with `ActiveSession` struct + per-session byte counters | `src/server/mod.rs` | **LOW** |
| H18 | **Stealth mode**: if `self.secret.is_some()`, only primary ALPN registered (no PAIRING_ALPN) | `src/server/mod.rs` | **MEDIUM** — changes pairing discovery behavior |
| H19 | **`bind_server`**: stealth mode detection; relay disabled → direct IP in ticket; `FsStore::load()` for blobs | `src/server/startup.rs` | **MEDIUM** |
| H20 | **`handle_transfer_stream`**: now takes `connection: iroh::endpoint::Connection` | `src/server/side_streams.rs` | **BREAKING** |
| H21 | **Blob transfer handler**: `handle_blob_put_request`, `handle_blob_get_request` | `src/server/transfer/files/blob.rs` (new) | **LOW** |
| H22 | **Host command**: Windows job object assignment; stealth mode display; `node_id` → `endpoint_id` in output | `cli/src/commands/host.rs` | **LOW** |

### 4.9 System / Service Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| SY9 | **Windows SCM rewrite**: `irosh_service_run()` with full Service Main function, `windows_service` crate | `src/sys/windows/service.rs` | **MEDIUM** — new service entry point |
| SY10 | **Job object**: child process cleanup via `AssignProcessToJobObject` | `src/sys/windows/job.rs` (new) | **LOW** |
| SY11 | **File-based logging**: `daemon.log` under state directory | `src/sys/windows/service.rs` | **LOW** |
| SY12 | **HOME/USERPROFILE remapping**: for LocalSystem runs | `src/sys/windows/service.rs` | **LOW** |
| SY13 | **System command**: sessions display via `Ui::session_table()` | `cli/src/commands/system.rs` | **LOW** |
| SY14 | **Windows UAC elevation**: auto-elevate via `Start-Process -Verb RunAs` | `cli/src/commands/system.rs` | **LOW** |

### 4.10 IPC Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| IP11 | **`DaemonStatus` includes `sessions: Vec<SessionStatus>`**: per-session byte counters | `src/server/ipc.rs` | **LOW** |
| IP12 | **Wormhole failure**: immediate burn on failure event (via `failure_rx`) rather than count-based | `src/server/mod.rs` | **MEDIUM** — behavior change |
| IP13 | **Shutdown drops IPC signal**: `let _ = ipc_shutdown_tx.send(()).await` | `src/server/mod.rs` | **LOW** |

### 4.11 Transfer Flow

| # | Change | Location | Risk |
|---|--------|----------|------|
| TR19 | **Blob frame kinds**: 18-20 added (`BlobPutRequest`, `BlobGetRequest`, `BlobGetReady`) | `src/transport/transfer/types.rs`, `codec.rs` | **WIRE-INCOMPATIBLE** — old peers reject kinds 18-20 |
| TR20 | **handle_transfer_stream**: `(stream, shell_state)` → `(connection, stream, shell_state)` | `src/server/transfer/mod.rs` | **BREAKING** |
| TR21 | **Blob put/get dispatch**: new handler arms | `src/server/transfer/mod.rs` | **LOW** |
| TR22 | **Client download**: `get_file()` / `get_file_with_progress()` → `download()` / `download_with_progress()` / `download_blob()` | `src/client/transfer/files/download.rs` | **BREAKING** — old methods renamed |
| TR23 | **Client upload**: `put_file()` / `put_file_with_progress()` → `upload()` / `upload_with_progress()` / `upload_blob()` | `src/client/transfer/files/upload.rs` | **BREAKING** — old methods renamed |
| TR24 | **ConnectionShellState::new()`**: takes `blobs` param | `src/server/transfer/state.rs` | **BREAKING** |

### 4.12 Config / Passwd / Check / Dashboard

| # | Change | Location | Risk |
|---|--------|----------|------|
| CF8 | **Config**: `Ui::error` signature change (tip param) | `cli/src/commands/config.rs` | **MEDIUM** — compile error if not updated |
| CF9 | **Check**: stealth status text changed; error messages reformatted with hints | `cli/src/commands/check.rs` | **LOW** |
| CF10 | **diagnostic.rs**: `node_id` → `endpoint_id`; `Endpoint::builder()` → `Endpoint::builder(presets::N0)`; key path `keys/node.secret` → `keys/endpoint.secret` | `src/diagnostic.rs` | **BREAKING** — field renames; behavior change |
| CF11 | **Dashboard**: uses `Ui::header()`, `Ui::success()`, `Ui::status()`; identity field `node_id` → `endpoint_id` | `cli/src/commands/dashboard.rs` | **LOW** |

### 4.13 Install / Uninstall

| # | Change | Location | Risk |
|---|--------|----------|------|
| IN11 | **install.ps1**: service upgrade awareness — stops service before overwrite, restarts after | `install.ps1:69-93` | **LOW** — new behavior |
| IN12 | **install.ps1**: interactive process detection; service restart logic | `install.ps1:86-104` | **LOW** |
| IN13 | **install.ps1**: service install guard (no double-install) | `install.ps1:131` | **LOW** |
| IN14 | **uninstall.ps1**: `schtasks` → `sc.exe` for service cleanup | `uninstall.ps1:56-57` | **MEDIUM** — v0.3.0 service installed as Task Scheduler won't be cleaned |
| IN15 | **uninstall.sh**: emoji → ASCII; TTY fix for non-TTY stdin | `uninstall.sh` | **LOW** |

---

## 5. Cross-Cutting Risk Register

### 5.1 Category: CRITICAL — Data Loss

| # | Version Jump | Change | Impact |
|---|-------------|--------|--------|
| ⚠️ R1 | v0.3→v0.4 | `SECRET_KEY_FILE` path: `keys/node.secret` → `keys/endpoint.secret` | **Existing users get a new identity on upgrade.** Old key is orphaned. If StateConfig path is clean, old identity is unrecoverable without manual file rename. |
| ⚠️ R2 | v0.3→v0.4 | `NodeIdentity` → `EndpointIdentity` rename | All code referencing identity types must update. Not data loss per se, but pervasive. |

### 5.2 Category: BREAKING — Protocol Incompatibility

| # | Version Jump | Change | Impact |
|---|-------------|--------|--------|
| ⚠️ R3 | v0.2→v0.3 | Side streams: metadata-first → magic-byte dispatch | v0.1/v0.2 clients cannot connect to v0.3+ servers for transfers/metadata. |
| ⚠️ R4 | v0.2→v0.3 | Windows IPC: Named Pipes → TCP loopback | v0.2 daemon and v0.3 client cannot communicate if mixed. |
| ⚠️ R5 | v0.3→v0.4 | Transfer frame kinds 18-20 added (VERSION=1 unchanged) | v0.3 and earlier peers reject blob frames. No version negotiation exists. |

### 5.3 Category: BREAKING — API/Signature Changes

| # | Version Jump | Change | Impact |
|---|-------------|--------|--------|
| ⚠️ R6 | v0.1→v0.2 | `ServerHandler::new` 4 args → 2 args | All server construction breaks |
| ⚠️ R7 | v0.1→v0.2 | `SessionEvent` variants renamed | All event consumers must update |
| ⚠️ R8 | v0.1→v0.2 | `ServerShutdown::close` consumes self | Callers with `&self` break |
| ⚠️ R9 | v0.2→v0.3 | `load_all_authorized_clients` return type changed | All trust consumers break |
| ⚠️ R10 | v0.2→v0.3 | `get_peer` → `load_peer` | Peer consumers break |
| ⚠️ R11 | v0.2→v0.3 | `ServerError::TransferFailed`/`ClientError::TransferFailed` fields changed | Error handling breaks |
| ⚠️ R12 | v0.2→v0.3 | `view_logs`/`query_service_status` signatures changed | Service callers break |
| ⚠️ R13 | v0.3→v0.4 | `dial_p2p` return 2-tuple → 3-tuple | All connect callers break |
| ⚠️ R14 | v0.3→v0.4 | `Credentials.password` `String` → `SecretString` | Password access pattern changes |
| ⚠️ R15 | v0.3→v0.4 | `&mut self` → `&self` on 10 Session methods | Refactor-safe but different ownership model |
| ⚠️ R16 | v0.3→v0.4 | Client `get_file`/`put_file` renamed to `download`/`upload` | All transfer callers break |
| ⚠️ R17 | v0.3→v0.4 | `Ui::error` signature changed (tip param) | All error callers must update |
| ⚠️ R18 | v0.2→v0.3 | `IpcResponse::Status` variant shape changed | All IPC consumers break |

### 5.4 Category: BEHAVIOR CHANGE — Subtle Breakage

| # | Version Jump | Change | Impact |
|---|-------------|--------|--------|
| ⚠️ R19 | v0.2→v0.3 | Wormhole blocked when vault populated + no password | Headless setups that relied on wormhole break |
| ⚠️ R20 | v0.2→v0.3 | Peer auto-save: interactive → silent | Connect flow changes; user loses awareness |
| ⚠️ R21 | v0.2→v0.3 | Peer add: duplicate names now rejected (prev. overwritten) | Scripts that replace peers break |
| ⚠️ R22 | v0.2→v0.3 | `_temp_password_hash` ignored → active | Changes wormhole auth behavior |
| ⚠️ R23 | v0.2→v0.3 | Dial timeout (20s + 3 retries) added | Connections that hung forever now fail fast |
| ⚠️ R24 | v0.2→v0.3 | Raw terminal: `libc` → crossterm | Different edge cases on non-Linux |
| ⚠️ R25 | v0.2→v0.3 | Input engine: pass-through → smart processing | All escape sequence / input behavior changes |
| ⚠️ R26 | v0.3→v0.4 | Blob transfers replace stream transfers | New transfer path, different error semantics |
| ⚠️ R27 | v0.3→v0.4 | Stealth mode: `secret` set → ALPN locked | Changes pairing discovery |

### 5.5 Category: Platform-Specific

| # | Version Jump | Change | Impact |
|---|-------------|--------|--------|
| ⚠️ R28 | v0.1→v0.2 | Linux service unit: `irosh-server.service` → `irosh.service` | Service appears "not installed" after upgrade |
| ⚠️ R29 | v0.1→v0.2 | Windows service: Task Scheduler → SCM | Old scheduled task orphaned |
| ⚠️ R30 | v0.3→v0.4 | Windows uninstall: `schtasks` → `sc.exe` | v0.3.0 service not cleaned |

---

## 6. Wire Protocol Compatibility

| Aspect | v0.1.0 | v0.2.0 | v0.3.0 | v0.4.0 |
|--------|--------|--------|--------|--------|
| Magic bytes (`IRFT`) | ✓ | ✓ | ✓ | ✓ |
| Protocol VERSION | 1 | 1 | 1 | 1 |
| Frame kinds supported | 1-13 | 1-17 | 1-17 | 1-20 |
| Metadata accept | Sequential | Sequential | Magic-byte dispatch | Magic-byte dispatch |
| Blob transfer | — | — | — | ✓ (kinds 18-20) |

**Critical finding**: The `VERSION` constant was never bumped despite adding new frame kinds. v0.4.0 clients sending blob frames (kinds 18-20) to v0.3.0 servers will get `UnsupportedKind` errors. There is no capability negotiation.

---

## Appendix: Quick Reference by Flow

### Which files changed in each version jump for each flow:

| Flow | v0.1→v0.2 | v0.2→v0.3 | v0.3→v0.4 |
|------|-----------|-----------|-----------|
| **Auth** | 8 files (new subsystem) | 4 files | 3 files |
| **Identity** | 2 files | 1 file | 4 files (BREAKING) |
| **Trust** | 1 file | 1 file | — |
| **Wormhole** | 4 files (new) | 2 files | 1 file |
| **Peer** | 1 file | 3 files | 1 file |
| **Connect/Session** | 10 files (rewrite) | 15 files (input engine) | 8 files (mutex + blobs) |
| **Host/Server** | 6 files (rewrite) | 4 files (BREAKING side streams) | 5 files (Router architecture) |
| **System/Service** | 8 files (new) | 3 files (signature changes) | 2 files (SCM rewrite) |
| **IPC** | 2 files (new) | 4 files (BREAKING transport) | 1 file |
| **Transfer** | 5 files (BREAKING) | 6 files (BREAKING) | 5 files (blobs) |
| **Config/Passwd/Check** | 3 files (new) | 3 files | 2 files |
| **Dashboard** | 1 file (new) | 1 file | 1 file |
| **CLI Dispatch** | 3 files (rewrite) | 3 files | 2 files |
| **Install/Uninstall** | 4 files | — | 2 files |
