# Changelog

All notable changes to the `irosh` project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-05-10

### Added
- **Structured Transfer Errors**: Replaced string-based error checking with a robust `TransferFailureCode` protocol for precise error reporting (NotFound, PermissionDenied, IsDirectory).
- **Interactive Parity**: Restored legacy V1 `~`-prefixed aliases (`~put`, `~get`, `~lls`, etc.) to the local command prompt.
- **Enhanced Tab Completion**: Fixed and optimized path completion for both local and remote filesystems.
- **Shortcut Connect Hardening**: Improved target parsing to reliably distinguish between tickets, aliases, and wormhole codes.

### Changed
- **Codebase Hardening**: Performed a total audit, pruning over 500 lines of dead code and stale `#[allow(dead_code)]` attributes.
- **Zero-Warning Compliance**: Achieved 100% Clippy compliance with warnings-as-errors across the entire workspace.
- **Dependency Optimization**: Pruned unused crates like `futures` and `futures-lite` to reduce binary size and compilation time.

### Fixed
- **Recursive Path Logic**: Resolved a regression where relative paths were incorrectly reported during recursive directory uploads.
- **CLI Prompt UX**: Fixed the "silent help" bug where unrecognized commands would trigger the help screen without an error message.
- **Type-Safe Connection**: Resolved nested result flattening bugs in the SSH handshake timeout logic.

## [0.2.0] - 2026-05-07

### Added
- **Unified CLI Consolidation**: Replaced the fragmented 3-binary architecture with a single, professional `irosh` binary.
- **Wormhole Rendezvous**: Implemented human-friendly ad-hoc pairing codes for secure peer discovery without tickets.
- **Unified Authenticator**: Introduced a master security engine with live-reloading support for authorized keys and passwords.
- **Wormhole Rate Limiting**: Added mandatory rate-limiting that burns the pairing session after 3 failed password attempts to prevent brute-force attacks.
- **Daemon-First Architecture**: Enabled IPC-based background control, allowing the CLI to orchestrate the system service seamlessly.
- **Cross-Platform Service Management**: Unified background service installation for Linux (systemd), macOS (launchd), and Windows.
- **Visual Progress Bars**: Integrated `indicatif` for high-end progress bars during file transfers.
- **Mid-Transfer Cancellation**: Added `Ctrl+C` interrupt support to safely abort file transfers.
- **Professional Documentation**: Completed full rustdoc synchronization and professional README refactoring for crates.io.

### Changed
- **Fat Library Refactor**: Isolated all core logic into the `irosh` crate, ensuring the CLI is a thin, deployable wrapper.
- **Non-Blocking I/O**: Migrated the client to a true non-blocking `AsyncFd` architecture for terminal handling.
- **Lazy Channel Initialization**: SSH channels are now opened on-demand, resolving multiplexing race conditions.

### Fixed
- **The "Deaf Daemon" Bug**: Resolved the issue where the background service would ignore IPC commands from the CLI.
- **Wormhole Lifecycle**: Pairing codes are now burned immediately upon successful authentication.
- **Graceful Pairing Exit**: Fixed the bug where the server would exit prematurely before the SSH session was fully established.
- **Exit Hangs**: Resolved multiple edge cases where the process would hang during terminal cleanup.
- **Windows PTY Stability**: Resolved PTY hangs and enabled reliable raw mode handling in the CLI.

## [0.1.0] - 2026-04-20

### Initial Release
- **Secure P2P Shell**: SSH sessions over Iroh peer-to-peer transport.
- **TOFU Security**: Trust On First Use policy for host keys and identities.
- **Recursive Transfers**: Initial implementation of recursive file and directory transfers.
- **Port Forwarding**: Basic local port forwarding capabilities.
