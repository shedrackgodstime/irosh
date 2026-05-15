# Changelog

All notable changes to the `irosh` project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-05-15

### Added
- **RAII Terminal Guard**: Introduced `TerminalGuard` to guarantee terminal state restoration (echo/raw mode) even during panics or crashes.
- **IPC Shutdown Synchronization**: The daemon control listener now shuts down gracefully alongside the main server loop, preventing "zombie" listeners and stale socket files.
- **Windows ACL Hardening**: Implemented secure, non-inherited ACLs for persistent storage on Windows, matching Unix `0600` privacy standards.
- **Structured Transfer Errors**: Replaced string-based error checking with a robust `TransferFailureCode` protocol for precise error reporting (NotFound, PermissionDenied, IsDirectory).
- **Interactive Parity**: Restored legacy V1 `~`-prefixed aliases (`~put`, `~get`, `~lls`, etc.) to the local command prompt with full history and path completion.

### Improved
- **Server Loop Architecture**: Refactored the core select-loop for better maintainability and eliminated redundant state checks in the Wormhole pairing flow.
- **Shell Namespace Integration**: Hardened the Linux `nsenter` and Windows PEB-walking logic for more reliable CWD resolution during file transfers.
- **Memory Safety**: Fixed potential memory leaks in the Windows SID allocation path and null-pointer dereferences in security error handlers.
- **Usage Feedback**: Added professional usage hints for interactive commands (`put`, `get`, `lcd`) when invoked without arguments.
- **Wormhole Auto-Save**: Restored seamless peer identity resolution and silent auto-saving for new connections.
- **Documentation "De-AI"**: Performed a global polish to remove AI-generated punctuation artifacts and ensure a human, authoritative voice.
- **Liability Hardening**: Synchronized the "Ironclad Disclaimer" across all public-facing documentation.
- **Enhanced Tab Completion**: Fixed and optimized path completion for both local and remote filesystems.

### Changed
- **Codebase Hardening**: Performed a total audit, pruning over 1,200 lines of dead code and stale legacy artifacts.
- **Zero-Warning Compliance**: Achieved 100% Clippy compliance with warnings-as-errors across the entire workspace.
- **Documentation Hygiene**: Moved internal planning and design documents to `docs_dev/` to keep the public repository clean.

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
