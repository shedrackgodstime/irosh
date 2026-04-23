# Changelog

All notable changes to the `irosh` project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-04-23

### Added
- **Visual Progress Bars**: Integrated `indicatif` for high-end progress bars during `:put` and `:get` transfers.
- **Mid-Transfer Cancellation**: Added `Ctrl+C` interrupt support to immediately abort file transfers and return to the shell.
- **P2P Dialing Spinner**: Added a dynamic visual indicator during the initial connection handshake.
- **Manager Identity Command**: Added `irosh identity` to easily retrieve and display the local Peer ID.
- **Persistent Command History**: CLI commands starting with `:` are now saved to `~/.irosh/client_history`.
- **Local Tab Completion**: Added filesystem completion for the `:put` command in the interactive shell.
- **Enhanced Help Menu**: Comprehensive `:help` and `:?` guides with examples and flag descriptions.

### Changed
- **Non-Blocking I/O Refactor**: Migrated the client to a true non-blocking `AsyncFd` architecture for stdin, eliminating background thread hangs.
- **Lazy Channel Initialization**: SSH channels are now opened on-demand, resolving multiplexing race conditions.
- **Architecture Hardening**: Centralized low-level terminal and signal logic into the core library.

### Fixed
- **Exit Hang**: Resolved the issue where the process would wait for a final Enter key before exiting.
- **PID Context**: Fixed PTY path resolution issues related to shell context inheritance.

## [0.1.0] - 2026-04-20

### Initial Release
- **Secure P2P Shell**: SSH sessions over Iroh peer-to-peer transport.
- **TOFU Security**: Trust On First Use policy for host keys and identities.
- **Recursive Transfers**: Initial implementation of recursive file and directory transfers.
- **Port Forwarding**: Basic local port forwarding capabilities.
