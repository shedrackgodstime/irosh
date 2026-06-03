# Changelog

All notable changes to irosh are documented here.

## [0.5.0] — Unreleased

### Added
- Metadata frame protocol fuzz target (`fuzz_metadata`)
- Fuzz targets now run 30s each in CI (was compile-only)
- `cargo hack --each-feature` CI job to verify feature flags are additive
- `#[tracing::instrument]` spans on all public async API functions (~25 functions)
- `tracing::error!()` at 15 critical failure boundaries (was 1)
- `tracing::trace!()` for wire-level events in transfer and metadata codecs
- SSH session lifecycle integration test (PTY → shell → resize → exec → disconnect)
- `#[inline]` on 27 hot-path codec functions
- End-to-end 4MB transfer pipeline benchmark
- SSH handshake and authentication benchmark
- Coverage gate in CI (`--fail-under-lines 80`)
- `ServerHandler`, `ConnectionShellState` made public for custom integrations
- `ServerReady::new()` public constructor
- Runtime metrics counters (`Metrics`, `MetricsSnapshot`) for connections, transfers, errors
- `Server::metrics()` accessor to query live counter values
- Metrics wired into `SshProtocol::accept` (connection tracking), `ServerHandler` (auth failure tracking), and `handle_transfer_stream` (transfer lifecycle tracking)
- `ServerHandler::with_metrics()` constructor for custom integrations
- Heap profiling example (`examples/heap_profile.rs`) using `dhat` for codec allocation analysis

### Fixed
- `disconnect()` is now idempotent (no-op when already in terminal state)
- `check_open()` guard on all `Session` methods to prevent use after disconnect
- Path traversal protection in `resolve_path()` (rejects `..` for relative paths)
- `PeerMetadata` sanitization (strips control chars, truncates to 255 chars)
- `validate_peer_name()` rejects null bytes, reserved Windows names, empty names
- `sanitize_remote_path()` path traversal detection
- `ensure_channel()` double-checked locking race eliminated
- All 17 public API soundness audit findings resolved

### Changed
- `PeerMetadata::current()` uses sanitizing constructor
- `read_metadata()` deserializes through sanitizing constructor
