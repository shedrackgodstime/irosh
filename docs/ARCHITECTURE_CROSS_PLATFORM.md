# Cross-Platform Architecture Strategy

Irosh is designed to run on Linux, macOS, and Windows. However, systems programming (especially dealing with Terminals, PTYs, and OS-level Daemons) varies wildly between Unix and Windows.

To prevent the codebase from becoming a tangled mess of `#[cfg(target_os = "windows")]` macros scattered randomly across files, we enforce the **Platform Isolation Pattern**.

---

## 1. The Core is Platform-Agnostic

The vast majority of the core `irosh` library must compile on any operating system without modification.

This includes:
- Iroh P2P Networking
- Authentication & Ticket Validation
- Cryptography (Ed25519)
- State Synchronization & File Watching (`notify` crate handles OS differences internally)
- Config Parsing

**Rule:** You should rarely, if ever, see `#[cfg(unix)]` inside `src/auth.rs` or `src/transport/`.

---

## 2. The `sys` Module (Platform Isolation)

Any code that directly interacts with OS-specific APIs must be isolated inside a dedicated `sys` module.

```rust
// In src/sys/mod.rs

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub mod windows;

// Re-export the active platform's implementation
#[cfg(unix)]
pub use unix::pty::*;

#[cfg(windows)]
pub use windows::pty::*;
```

### Key Areas for Isolation:

1. **Pseudoterminals (PTY)**: 
   - Linux uses `termios`, `ioctl`, and `forkpty`.
   - Windows uses `ConPTY` and `CreateProcess`.
   - We define a common `PtySession` trait or struct, and the `sys::*::pty` modules implement it.

2. **System Services (Daemon Installation)**:
   - Linux uses `systemd`.
   - macOS uses `launchd`.
   - Windows uses `Windows Service Control Manager (SCM)`.
   - The CLI command `irosh system install` delegates the actual work to `sys::*::service::install()`.

---

## 3. Migration Strategy (The "Stub" Approach)

When migrating code or building new features for an OS you don't currently have access to (e.g., waiting for a Windows PC), **do not guess the implementation**.

Instead, build the Unix implementation, and create a **Stub** for Windows that safely returns a generic error.

```rust
// src/sys/windows/pty.rs

use crate::error::IroshError;

pub fn spawn_pty() -> Result<(), IroshError> {
    // TODO(windows): Implement ConPTY spawning here when Windows PC is available.
    Err(IroshError::PlatformNotSupported("PTY spawning is not yet implemented for Windows."))
}
```

This allows the entire project to compile and pass CI on all platforms, while clearly isolating the missing pieces for future implementation.
