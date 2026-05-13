# Irosh Terminal Polish & Intelligence Plan

This document outlines the next phase of Irosh development, incorporating "Intel" from world-class Rust projects: Zellij, Nushell, Ratatui, and Starship.

## 1. Intelligence: Remote Autocompletion (Nushell-inspired)
**Goal**: Make the `irosh>` prompt feel intelligent by predicting what the user wants.

- [ ] **Remote File Completer**: Implement a completion engine that queries the remote peer for file paths during `get <TAB>` or `cd <TAB>`.
- [ ] **Local File Completer**: Standardized completion for `put <TAB>`.
- [ ] **Context-Aware Hints**: Implement "Grayed-out" suggestions (fish-style) for previously used local commands.
- [ ] **Syntax Highlighting**: Highlight keywords (`put`, `get`, `lls`) and validate paths (red if missing, green if exists) in real-time.

## 2. Visuals: The Transfer Dashboard (Ratatui-inspired)
**Goal**: Provide a premium, full-screen monitoring experience for long-running transfers.

- [ ] **Alternate Screen Dashboard (`~F`)**:
    - **Peer Pane**: Live info on the remote peer (OS, version, latency).
    - **Transfer Pane**: Multi-line progress bars with speed graphs (using Ratatui's `Sparkline`).
    - **Connection Pane**: Real-time Iroh endpoint and relay statistics.
- [ ] **Async UI Updates**: Use a dedicated UI task that receives state updates from the P2P engine without blocking the main event loop.

## 3. Aesthetics: The Status Line (Starship-inspired)
**Goal**: High information density with premium visuals.

- [ ] **Rich Prompt**: Incorporate icons (Nerd Fonts) for:
    - Remote OS (Windows/Linux/Android icons).
    - Connection Security (Locked/Unlocked icons).
    - Git Status (if the remote CWD is a git repo).
- [ ] **Unified Status API**: Create a module inspired by Starship's `hostname.rs` and `git_status.rs` to format our remote metadata.

## 4. Reliability: Asynchronous Core (Zellij-inspired)
**Goal**: Zero-lag, zero-corruption terminal state.

- [ ] **Input Rewriting**: Ensure all terminal sequences (CSI, OSC) are parsed and handled consistently.
- [ ] **Terminal Emulation Light**: Track the cursor position locally to handle "Reflowing" when the window resizes, preventing line-wrap artifacts.
- [ ] **Backpressure Handling**: Ensure large amounts of remote data (e.g., `cat`ing a huge file) don't lag the local UI.

---
**Reference Intel Used**:
- `zellij-utils/src/input`: Handling complex actions.
- `nu-cli/src/completions`: File and command suggestions.
- `ratatui/examples/apps/async-github`: Managing UI state in an async environment.
- `starship/src/modules`: Informational modules for host/git/status.
