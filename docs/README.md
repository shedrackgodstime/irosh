# Irosh documentation

Developer-facing reference for irosh. Irosh ships as two crates — the
`irosh` library (protocol, transport, security, storage) and the `irosh-cli`
(the interactive `irosh` binary). This document covers the architecture, the
wire protocol, the security model, and the CLI.

For the library API, see the generated crate documentation on
[docs.rs](https://docs.rs/irosh/latest/irosh/), which embeds the library's
own `README.md` and rustdoc for every public item.

This document describes what exists, not a promise.

## Contents

1. [Architecture](#1-architecture)
   - [The two crates](#the-two-crates)
   - [Dependency direction](#dependency-direction)
   - [Library modules](#library-modules)
   - [Feature flags](#feature-flags)
   - [State layout](#state-layout)
   - [Traceability](#traceability)
2. [Core flows](#2-core-flows)
   - [Hosting](#hosting)
   - [Connecting](#connecting)
   - [File transfers](#file-transfers)
3. [Protocol](#3-protocol)
   - [Transport and ALPNs](#transport-and-alpns)
   - [Control channel](#control-channel)
   - [Transfer channel](#transfer-channel)
   - [Wormhole rendezvous](#wormhole-rendezvous)
   - [Invariants](#invariants)
4. [Security](#4-security)
   - [Identity](#identity)
   - [Authentication policy](#authentication-policy)
   - [Credential handling](#credential-handling)
   - [Rate limiting](#rate-limiting)
   - [Stored-state protection](#stored-state-protection)
   - [Threat model](#threat-model)
5. [CLI reference](#5-cli-reference)
   - [Invocation](#invocation)
   - [Global flags](#global-flags)
   - [Commands](#commands)
   - [Interactive session](#interactive-session)
   - [JSON output](#json-output)
6. [Project](#6-project)

---

## 1. Architecture

### The two crates

```
irosh/
├── src/    # irosh — core library
└── cli/    # irosh-cli — command-line interface
```

**irosh — the core library.** The core library contains the functionality
that makes up irosh. It is responsible for:

- peer-to-peer communication
- transfer protocols
- connection and session handling
- reusable application logic

The library is independent of the CLI and can be embedded by other
applications.

**irosh-cli — the command-line interface.** The CLI is the user-facing
layer. It is responsible for:

- argument and command parsing
- terminal interaction
- presenting library results
- wiring commands to the irosh library

The CLI does not own the core logic. It uses irosh as its underlying library.

### Dependency direction

```
┌──────────────┐
│  irosh-cli   │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│    irosh     │
└──────────────┘
```

The dependency flows in one direction: CLI → library. This keeps the core
implementation reusable and the CLI replaceable.

### Library modules

| Module       | Responsibility                                                          |
| :----------- | :---------------------------------------------------------------------- |
| `transport`  | Iroh endpoint setup, ALPN derivation, tickets, wormhole (Pkarr), transfer & metadata codecs |
| `server`     | SSH server: protocol loop, session handler, PTY, IPC daemon, transfers  |
| `client`     | SSH client: connect flow, session events, IPC client, transfer engine   |
| `session`    | Shared shell/PTY state used by both server and client                   |
| `auth`       | Key, password, and unified policy authenticators; pairing monitors      |
| `storage`    | Identity, trust vault, peer address book, node password, config         |
| `config`     | State/security/app configuration types                                  |
| `error`      | Typed error hierarchy mapped from the underlying crates                 |
| `metrics`    | Atomic connection/transfer/error counters                               |
| `diagnostic` | Filesystem, SSH-binary, and network checks (powered by `irosh check`) |
| `sys`        | Platform glue: raw terminal, PTY sizes, signals, service management     |

### Feature flags

Everything heavy is opt-in so the library stays lean for embedders:

- `server` — the SSH server, PTY handling, transfer serving
- `client` — the SSH client, interactive handlers, transfer engine
- `storage` — persistent identity/trust/peer state
- `transport` — the Iroh and Pkarr stacks

`default = ["server", "client"]`; the CLI enables all four. The halves nest:
`server` and `client` each pull in `storage` and `transport`. Keeping
`storage` and `transport` separate is what lets an embedder use, say,
persistence alone (serde-based, no network stack) without dragging in the
Iroh/Pkarr/blobs dependencies.

### State layout

State defaults to `~/.irosh/{client,server}` (override with the `IROSH_STATE`
env var). Inside:

```
keys/endpoint.secret   Ed25519 node identity
trust/clients/*.pub    authorized client (vault) keys
trust/servers/*.pub    known server host keys (TOFU)
shadow                 Argon2id node-password hash
peers/<name>.json      saved peer aliases
irosh.json             persistent config
blobs/                 staged transfer store (iroh-blobs)
ipc.port               daemon IPC port (server state only)
```

Writable files are written with restrictive permissions (0600 on Unix, ACLs
on Windows) and secret-bearing writes are atomic. See [Stored-state
protection](#stored-state-protection) for the details.

### Traceability

Every byte counting as observable behavior is instrumented: connection,
transfer, and error counters via `metrics`, `tracing` spans on public APIs,
and round-trip benches plus fuzz targets for the codecs. CI enforces a
workspace line-coverage floor (`cargo llvm-cov --fail-under-lines 50`); the
exact threshold lives in the workflow file, not here. It is measured.

## 2. Core flows

### Hosting

`irosh system install` registers a background service (systemd, launchd, or
the Windows SCM) that runs `irosh host`. `irosh host` binds the endpoint,
prints its ticket, and runs the accept loop. The service also exposes a tiny
IPC server (a port file plus commands) so the CLI can manage it over IPC.

### Connecting

`irosh <target>` resolves `target` as an alias → saved peer ticket →
wormhole code, dials it, negotiates SSH auth (see [Authentication
policy](#authentication-policy)), requests a PTY, and streams the shell.
`--exec` runs one remote command instead of a shell. While connected, the
[interactive session](#interactive-session) toolkit applies.

### File transfers

Transfers run over a dedicated framed stream, not through the shell. The
client sends a request (`put`/`get`) and the server stream chunks back or
forth until a completion marker. The server stages files in an iroh-blobs
store so partial or interrupted transfers don't corrupt targets. Wire-level
detail — framing, sequencing, recursive walks, and blob staging — is in the
[Transfer channel](#transfer-channel) section.

## 3. Protocol

### Transport and ALPNs

All networking rides on **QUIC** through the [Iroh](https://iroh.computer)
P2P stack. A host binds an Iroh endpoint and publishes a shareable **ticket**
(relay URLs + direct addresses + endpoint ID). A client dials that ticket,
which lets Iroh hole-punch direct or fall back to a relay — no open ports,
no public IP, no VPN required.

Three Application-Layer Protocol Negotiation values exist:

| ALPN               | Purpose                                        |
| :----------------- | :--------------------------------------------- |
| `irosh/1`          | Standard SSH session                          |
| `irosh/1/<hex>`    | Stealth session; `<hex>` = first 8 bytes of `sha256(secret)` |
| `irosh/pairing/v1` | One-shot wormhole "Trust-Seed" handshake      |

**Stealth listeners.** Stealth mode replaces the `irosh/1` ALPN with
`irosh/1/<hex>`. A scanner probing the endpoint sees neither an SSH banner
nor a recognizable protocol, so the listener is invisible without the shared
secret. The derived ALPN rides in the TLS ClientHello, which is plaintext to
a passive observer (no ECH), so a listener can be fingerprinted by its derived
ALPN even though the secret itself is never exposed. The endpoint still
advertises the Iroh stack's other ALPNs (including the blob store), so stealth
hides *what this endpoint is for*, not that an iroh endpoint is here. This is
best effort, not airtight.

### Control channel

Peers negotiate peer metadata (platform, capability hints) over a small
framed channel:

```
MAGIC "IRMD" | VERSION (u8 = 2) | KIND (u8) | LEN (u32) | PAYLOAD (LEN)
```

- Kinds: `1` metadata request, `2` peer metadata.
- Payloads are JSON (serde) and capped at 8 KiB.

### Transfer channel

File transfers run on their own framed stream, separate from the shell. All
frames share a header:

```
MAGIC "IRFT" | VERSION (u8 = 2) | KIND (u8) | LEN (u32) | PAYLOAD (LEN)
```

- Control payloads are capped at 8 KiB; data chunks are capped at 64 KiB.
- Kinds (20, contiguous `u8` constants; the list in the `transport::transfer`
  codec module is the source of truth): put request / ready / chunk /
  complete (1-4), get request / ready / chunk / complete (5-8), error (9),
  cwd request / response (10-11), exists request / response (12-13), new
  entry / entry complete (14-15), completion request / response (16-17),
  blob put request (18), blob get request / ready (19-20). Gaps are not
  reserved — a new wire operation appends to the list.

**Upload (`put`):**

```
PUT_REQUEST ──► PUT_READY          path, size, mode, recursive flag
PUT_CHUNK    ──► (repeated)        ≤ 64 KiB each
PUT_COMPLETE ◄──                   total size
```

`size` is the declared total and is enforced: the server rejects any
stream that exceeds it, and the completion frame must match the bytes
actually sent.

**Download (`get`):**

```
GET_REQUEST ──► GET_READY          size, mode
GET_CHUNK    ◄── (repeated)
GET_COMPLETE ──►
```

**Recursive transfers.** Each file within a tree is announced with a new-entry
header (relative path, size, mode, `is_dir`), followed by its data chunks,
then an entry-complete marker, before the next entry begins.

**Blob staging.** Transfers can stage through the iroh-blobs store. A blob
put request carries the path plus the content hash, format, and size; a blob
get request pulls by hash. The server writes the blob to a fresh path and
refuses to overwrite an existing one, so partial or interrupted transfers
cannot corrupt a target.

### Wormhole rendezvous

Rendezvous uses [Pkarr](https://pkarr.org) on the mainline DHT:

- A keypair is derived from the code: `sha256("irosh-wormhole-v1" ‖ code)`
  fed as the seed. The salt keeps irosh topics distinct from other Pkarr
  applications.
- The host signs a TXT record holding its ticket and publishes it under that
  key; the peer resolves the record and dials the ticket.
- Listing waits up to 5 minutes; records are republished every 60 seconds
  while active, and explicitly unpublished on close so lingering discovery
  dies.
- The pairing handshake itself runs over `irosh/pairing/v1` and admits the
  connecting key into the trust vault on success; the wormhole is burned
  after repeated failed attempts (see [Rate limiting](#rate-limiting)).

### Invariants

- Frame headers are magic-checked; a wrong magic aborts the stream.
- Payload limits are enforced on write and on read, so oversized chunks
  surface as typed `PayloadTooLarge` errors rather than silent truncation.
- Transfer entry paths are sanitized before use: null bytes are rejected, absolute
  entry names are refused, and Windows reserved names are handled. The resolved
  target path is used as given.

## 4. Security

### Identity

Each node has an Ed25519 node identity in its state directory. The public
side (endpoint ID) is what tickets advertise. Fingerprints are SHA-256 and
are the currency of the trust vault.

### Authentication policy

The library owns the *protocol* of authentication but never decides *how* to
validate credentials (C-CALLER-CONTROL): it calls an authenticator trait and
respects the result. The CLI ships four backends: key-only, password, combined,
and the unified policy — selectable via `irosh host --auth-mode`.

The `UnifiedAuthenticator` is the master policy. Precedence is strict:

1. **Established trust wins.** A key already in the vault (a `.pub` file
   under `trust/clients/`) is accepted outright.
2. **Node password challenges unknown keys.** A permanent password set via
   `irosh passwd`.
3. **Wormhole temp password overrides once.** The Invite Pattern admits the
   connecting key into the vault for that single pairing.
4. **TOFU only as last resort.** If the vault is empty *and* no password is
   set, the first device to connect becomes the permanent owner.

Worked example: vault empty, node password set, first device connects. Rules
1 and 3 don't apply (nothing vaulted, no wormhole). Rule 2 fires: the device
is challenged for the password. On success it is admitted — but it is *not*
vaulted, because TOFU (rule 4) is gated on "no password set." The second
device to connect must present the same password; the first device is not
silently promoted to owner.

Flip it: vault empty, no password, first device connects. Rules 1-3 all
miss, rule 4 fires, and that device becomes the permanent owner. From then
on, rule 1 does the work for that device and rule 2 (if a password is later
set) gates everyone else.

Key-only operation additionally honors `HostKeyPolicy`:

- `Strict` rejects unknown clients outright, even when the vault is empty.
- `Tofu` trusts the first client seen.
- `AcceptAll` accepts everyone (explicitly opt-in only).

### Credential handling

- Passwords are hashed with **Argon2id** and a random salt, never stored
  plaintext. The node hash lives in `shadow`.
- Verifying a password over SSH is done via the challenge to whatever key the
  connecting peer presented, so a password is never sent as part of a public
  key exchange.

### Rate limiting

Failed authentication attempts are throttled by a node-wide **decaying
60-second window**:

- The failure counter resets on any successful authentication.
- It also expires on its own after the window, so a single malicious client
  cannot permanently lock the node for everyone.
- Every backend enforces it: the key backend and the password backend each
  count three failures within the window before blocking further checks.
- On a wormhole, three failures within the window burn the pairing, and the
  dormant-period counter decays from there.

### Stored-state protection

- Writable files are written at `0600` and directories at `0700` on Unix.
  On Windows the equivalent is a DACL applied via `SetFileSecurityW`
  (`GENERIC_ALL` for the current user, plus SYSTEM and Administrators)
  rather than relying on default ACL inheritance.
- Secret-bearing writes are atomic: data is written to a temp file with
  strict permissions first, then renamed into place.

### Threat model

What the design defends against:

- Uninvited devices connecting (trust vault + rate limiting).
- Passive discovery of listeners in stealth mode (see
  [Transport and ALPNs](#transport-and-alpns)).
- Reading the node password without filesystem access (Argon2id).
- Credential brute force (decaying lockout).

What it does not claim to defend against:

- A relay operator learning *metadata* (who dials whom, when) — QUIC encrypts
  content, not the existence of a connection.
- Anyone with read access to the state directory: the identity secret is
  protected by file permissions, not encryption at rest.
- Compromised entry points in the transfer/session code — that is what the
  fuzz targets, metrics, and coverage gate exist to find, not to have already
  solved.

## 5. CLI reference

### Invocation

```
irosh [GLOBAL FLAGS] <command> [ARGS]
irosh [GLOBAL FLAGS] <target>        # shortcut for `irosh connect <target>`
```

Subcommand names win over the shortcut: `irosh status` runs `irosh check`
(the `status` alias), not `irosh connect status`. The shortcut only fires
when the first non-flag argument is not a known subcommand — so to connect
to a peer whose alias collides with a subcommand, use `irosh connect
<target>` explicitly.

### Global flags

| Flag            | Meaning                                              |
| :-------------- | :--------------------------------------------------- |
| `--state <dir>` | Override the state directory (also honored via `IROSH_STATE`) |
| `-v, --verbose` | Debug logging for `irosh` crates                     |
| `--log <level>` | Log-level override (e.g. `debug`, `trace`)           |
| `--json`        | Machine-readable JSON output for automation          |
| `-y, --yes`     | Auto-confirm all danger prompts                      |

### Commands

#### connect

Connect to a remote peer for an interactive shell.

```
irosh connect [target] [--code <code>] [--ticket <ticket>] [-L local:remote]
              [-s <secret>] [-e <command>]
```

- `<target>` may be a saved alias, a ticket string, or a wormhole code.
- `--code` / `--ticket` force the resolution path explicitly.
- `-L local:remote` opens a local port forwarded to a remote address over the
  session.
- `-s <secret>` enables stealth mode (hashed ALPN shared with the host).
- `-e <command>` runs one remote command and exits, printing its output.

With no argument, prompts for a code or ticket.

#### host

Run the server in the foreground (temporary sessions or debugging; use
`system start` for background hosting).

```
irosh host [-s <secret>] [--auth-mode <mode>] [--authorize <keyfile>]
           [--simple] [--idle-timeout <seconds>]
```

- `--auth-mode` forces `key`, `password`, `combined`, or `unified` (default).
- `--authorize` pre-vaults an SSH public key before first connect (headless setup).
- `--simple` prints machine-readable hints instead of framed output.
- `--idle-timeout` disconnects idle shells after N seconds.

#### wormhole

Start or manage discovery wormholes.

```
irosh wormhole [code] [-p, --passwd] [--persistent]
```

- No `code` generates a random pairing code.
- `code = status` reports the current wormhole; `disable` turns it off.
- `-p` adds a one-time session password (Invite Pattern).
- `--persistent` keeps the wormhole across reboots.

#### system

Background daemon management.

```
irosh system <install|uninstall|start|stop|restart|status|logs [--follow]>
```

Installs a service (systemd, launchd, or the Windows SCM) running `irosh
host`.

#### peer

Address book.

```
irosh peer <list|add <name> <target>|remove <name>|info <name>|rename <old> <new>>
```

#### trust

Authorized-client vault.

```
irosh trust <list|revoke [fingerprint]|reset>
```

`revoke` accepts a fingerprint or prefix; with neither, it offers an
interactive picker. `reset` clears the vault.

#### passwd

Node-password management.

```
irosh passwd <set|remove|status>
```

The node password challenges unknown keys and unlocks wormhole pairing.

#### identity

```
irosh identity <show|rotate>
```

`show` prints the node identity and fingerprints; `rotate` generates a new
identity.

#### config

```
irosh config <list|get <key>|set <key> <value>|export [--output <file>]|import <file>>
```

Persisted settings from the state directory's `irosh.json`.

#### check

Health checks, alias `status`:

```
irosh check
```

Reports filesystem permissions, SSH binary presence, and network reachability
(endpoint online/offline, NAT type, relay URLs).

### Interactive session

While connected, a line whose first byte is `~` is treated as an escape:

| Escape | Action                                      |
| :----- | :------------------------------------------ |
| `~.`   | Terminate the connection                    |
| `~C`   | Open the local command prompt               |
| `~?`   | Show this help                              |
| `~~`   | Send a literal `~` (OpenSSH parity)         |

From the `~C` prompt, local commands:

| Command                              | Meaning                                    |
| :----------------------------------- | :----------------------------------------- |
| `help`, `?`                          | List local commands                        |
| `lpwd`, `pwd`                        | Print local working directory              |
| `lls`, `ls`                          | List a local directory                     |
| `lcd`, `cd`                          | Change local directory                     |
| `paths`                              | Show resolved transfer path rules          |
| `put [-r] <local> [remote]`          | Upload a file or directory                 |
| `get [-r] <remote> [local]`          | Download a file or directory               |
| `clear`, `cls`                       | Clear the local terminal                   |
| `disconnect`                         | Terminate the connection                   |
| `exit`                               | Exit the local prompt                      |

Escape sequences are also recognized from the raw session without entering
the prompt, so `~.` works anywhere.

### JSON output

With `--json`, results are wrapped in a stable envelope:

```json
{ "ok": true, "data": { ... }, "error": null }
{ "ok": false, "data": null, "error": { "message": "...", "code": "..." } }
```

The informational commands emit this envelope in JSON mode:
`check`, `host`, `identity`, `passwd`, `peer list`, `peer info`,
`system`, `trust`, and `wormhole`. The remaining one is `connect`,
which streams an interactive terminal and has no wrapping result. Scripted sessions should use
`connect --exec` instead.

## 6. Project

- **Wire protocol versions.** Control and transfer frames both carry
  `VERSION (u8)`. The current value is `2`; `1` is rejected, and there is no
  in-band version negotiation yet — a peer that doesn't speak the host's
  version is dropped at the first frame.
- **MSRV.** Declared as `rust-version = "1.85"` in `Cargo.toml`. The
  `rust-toolchain.toml` pins the development toolchain instead, and no
  CI job checks the MSRV.
- **License.** MIT OR Apache-2.0.