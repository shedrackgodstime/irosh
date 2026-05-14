# 🎨 Irosh UX & Terminal Guidelines

This document defines the **User Experience (UX)** and **Terminal Rendering Philosophy** for Irosh. It focuses on the *goals* we want to achieve. Technical implementations must adapt to these goals, not the other way around.

---

## 1. The Core Philosophy: "True Transparency"
Irosh should feel like a native, invisible extension of your existing terminal, not a separate "app" running inside it.

- **Seamless Integration:** Moving from a remote shell to a local Irosh command should feel as natural as typing a local alias.
- **Scrollback Integrity:** Every interaction—remote commands, local file transfers, and the `irosh>` prompt—must be preserved in the terminal's permanent scrollback.
- **No Screen Hijacking:** We never clear the screen or use temporary buffers that hide the user's previous work. The remote session context is sacred.

## 2. Visual Persistence & Context
The user should never lose their "place" in a session.
- **Context Awareness:** When a user initiates a local command (like `~put`), the remote prompt that triggered it must remain visible and uncorrupted.
- **Permanent Records:** Local outputs (transfer summaries, status reports) are first-class citizens of the terminal history. They shouldn't vanish once a command completes.

## 3. Non-Destructive Interaction
Local UI elements must coexist with complex remote environments without breaking them.
- **Zero Corruption:** Local input echoes must never overwrite or "clobber" the characters printed by the remote shell (e.g., multi-line prompts or diagnostic messages).
- **Graceful Transitions:** Switching between local and remote modes must be visually stable. No flickering, no sudden cursor jumps, and no unexpected line clearing.

## 4. The "Forward-Only" Flow
To ensure stability across all terminal types (from high-end emulators to basic serial consoles), Irosh follows a "Forward-Only" rendering logic.
- **Append-Only UI:** Treat the terminal as an infinite, downward-scrolling stream.
- **Avoid "Teleportation":** We avoid complex logic that "guesses" the cursor's absolute position. Instead, we use relative movements that work universally.
- **Clean Handoffs:** When a local task ends, the cursor is left in a predictable, ready-to-use state on a fresh line, allowing the remote shell to resume naturally.

## 5. Professional Aesthetics & Hygiene
The Irosh interface must be "tight" and professional.
- **Efficient Spacing:** No wasted vertical space. Avoid redundant blank lines that push useful information off the screen.
- **Visual Discipline:** Every line printed should be meaningful. Progress bars and prompts should be surgical and minimalist.

## 6. The Platform Matrix
While cross-platform compatibility is essential, inherent OS limitations dictate the following target states:
- **Linux (Server/Client):** Full feature parity. Seamless stream-based PTYs with zero screen clearing or corruption.
- **Windows (Server):** Best-effort. Due to ConPTY's absolute 2D coordinate grid, out-of-band scrolling will cause desynchronization. Irosh uses surgical mitigations (like CPR-based `\x0C` tracking) to resolve this, but users may observe forced remote screen clears when scrolling occurs. This is an accepted OS limitation.
- **Windows (Client):** High stability. Follows the same stream-based principles as the Linux client when connecting to Linux servers.

---

**⚠️ Directive:** Any changes to the terminal transport or prompt logic must be measured against these goals. If a fix solves a bug but violates "True Transparency" or "Scrollback Integrity," it is an invalid fix.
