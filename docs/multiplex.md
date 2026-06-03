# Connection Multiplexing

Allow multiple local terminals to share one remote SSH session — like SSH `ControlMaster auto`.

## UX

```
irosh connect my-server          # 1st terminal: connection + shell (auto-holder)
irosh connect my-server          # 2nd terminal: auto-attach via IPC
irosh connect --fresh my-server  # Force a new independent connection
irosh sessions                   # List active connections
irosh disconnect my-server       # Close connection + all channels
```

No config files, no flags for basic use. First `connect` automatically becomes the
"holder." Subsequent `connect`s to the same peer auto-detect and attach.

## Architecture

**First-terminal-as-holder** (like tmux, not like ControlMaster's separate process)

| Component | What it does |
|---|---|
| `drive_session` (existing) | Extended to also run an IPC server for attach requests |
| IPC socket | `<state>/sessions/<peer_hash>/session.sock` |
| New channel relay | IPC client connects, holder opens SSH channel, bi-directional stream copy |
| Session registry | `<state>/sessions/<peer_hash>/metadata.json` — peer ID, ticket, timestamp |

Why not the server daemon? The server daemon (`irosh system start`) manages
incoming connections. Adding outgoing client multiplexing to it mixes concerns.
First-terminal-as-holder keeps each connection independent.

## Changes

### 1. Library: `ClientSessionHolder` (`src/client/holder.rs` — new)

```rust
pub struct ClientSessionHolder {
    ipc_path: PathBuf,
    session: Arc<Session>,
    cancel: CancellationToken,
}

impl ClientSessionHolder {
    pub async fn listen(self) -> Result<()>;
    pub fn exists(state: &StateConfig, peer_id: &str) -> bool;
    pub fn socket_path(state: &StateConfig, peer_id: &str) -> PathBuf;
}
```

`listen()` loop:
1. Accept IPC connections
2. Read `AttachRequest` (single byte)
3. Call `session.open_additional_channel()`
4. Call `channel.into_stream()`
5. Spawn `tokio::io::copy_bidirectional(socket, channel_stream)`
6. On disconnect, close channel

### 2. Library: `Session::open_additional_channel()` (`src/client/mod.rs`)

```rust
impl Session {
    pub async fn open_additional_channel(&self) -> Result<russh::Channel<client::Msg>> {
        let handle = self.handle.read().await;
        handle.channel_open_session().await
    }
}
```

The handle is `Arc<RwLock<client::Handle>>` — already shared, so multiple
channels can be opened concurrently.

### 3. CLI: connect auto-detection (`cli/src/commands/connect/mod.rs`)

After resolving target, before establishing:

```
if holder exists for this peer → attach to existing session
else → establish connection as usual
```

Attach path connects to the holder's IPC socket, sends `AttachRequest`,
then runs `drive_attached_session(ipc_stream, input_engine)` — identical
shell loop but reading from the IPC stream instead of a `Session`.

### 4. CLI: drive_session as holder (`cli/src/commands/connect/session.rs`)

After connection established, before shell loop:

```rust
if should_become_holder {
    let holder = ClientSessionHolder::new(session.clone(), &state, peer_id);
    tokio::spawn(holder.listen());
}
```

Existing `drive_session` loop continues unchanged. IPC attach requests
are handled by the spawned holder task (opens new SSH channels and relays
I/O between IPC sockets and channel streams).

### 5. CLI: `irosh sessions` (`cli/src/commands/sessions.rs` — new)

List active holders: scan `<state>/sessions/*/metadata.json`, show peer,
ticket, uptime, channel count.

### 6. CLI: `irosh disconnect` (`cli/src/commands/disconnect.rs` — new)

Send shutdown byte to holder's IPC socket. Holder closes all channels,
closes P2P connection, cleans up session directory.

## IPC Protocol

Simple byte-pipe over Unix socket (TCP on Windows):

```
Client → Holder:  [0x01]        AttachRequest
Holder → Client:  [0x00]        AttachAccepted
--- bidirectional raw I/O follows ---
On EOF: close channel + socket
```

No structured messages — after handshake, raw bytes flow both ways.

## Files to create/modify

| File | Action |
|---|---|
| `src/client/holder.rs` | **New** — `ClientSessionHolder` |
| `src/client/mod.rs` | **Modify** — add `open_additional_channel()` |
| `src/lib.rs` | **Modify** — export new types |
| `cli/src/commands/connect/mod.rs` | **Modify** — auto-detect + attach |
| `cli/src/commands/connect/session.rs` | **Modify** — holder mode |
| `cli/src/commands/sessions.rs` | **New** — `irosh sessions` |
| `cli/src/commands/disconnect.rs` | **New** — `irosh disconnect` |
| `cli/src/commands/mod.rs` | **Modify** — register commands |
| `cli/src/main.rs` | **Modify** — maybe none, subcommands handle it |

## Implementation order

1. `ClientSessionHolder` + `open_additional_channel()` — library core
2. Connect auto-detection in CLI — wiring
3. `drive_session` holder mode — extension
4. `sessions` + `disconnect` commands — polish
5. `--fresh` flag — escape hatch

## Open questions

- **PTY per attached channel** — each gets its own PTY (standard SSH behavior)
- **Channel close on EOF** — attached terminal disconnects → channel closes, main
  connection stays alive (natural with `channel.eof()`)
- **disconnect semantics** — closes the master connection, which terminates all
  channels (SSH-like)
