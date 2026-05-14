# 🤝 Handover for New Irosh Session: Terminal Stabilization

## 🚩 Status Report
We have spent 1 week trying to stabilize the terminal input/output for `irosh` across Linux and Windows. We have identified that the "hacking" approach (manually parsing raw bytes and counting cursor positions) is fundamentally broken for Windows ConPTY and complex remote prompts.

## 🧠 Key Learnings & Intel
1.  **Windows ConPTY is stateful**: It tries to "help" with cursor movements. If we send relative backspaces (`\x08`), it often gets out of sync, leading to the "Corruption" bugs.
2.  **Tab Corruption**: Current Tab completion fails because it doesn't account for the unknown length of the remote prompt. It "clobbers" the screen.
3.  **The bssh/WezTerm Pattern**: Professional projects use a **Double-Buffer** or **Event-Driven** model for local prompts. They don't use raw bytes for local editors; they use structured events (`crossterm::event`).
4.  **True Transparency**: We must respect the terminal's scrollback. We never use Alternate Screen buffers for the `irosh>` prompt.

## 🚀 The Path Forward (Permanent Solution)
The new architectural plan is documented in `docs/PERMANENT_TERMINAL_SOLUTION.md`.

### Immediate Next Steps for the New AI:
1.  **Refactor `input.rs`**: Replace the `InputEngine::process_local` loop with a state machine that uses `crossterm::event::read()` only when the mode is `LocalEdit`.
2.  **Implement "Forward-Only Redraw"**: Stop using `\x08`. Use `\x1b[G\x1b[K` followed by the full prompt redraw. This is the only way to satisfy the Windows ConPTY stability requirement.
3.  **Implement Persistent History**: Save to `~/.irosh_history`.
4.  **Adopt the "Nuclear Cleanup"**: Ensure the cursor and terminal modes are reset whenever exiting the prompt.

## 📂 Relevant Files
- `docs/UX_GUIDELINES.md`: The bedrock philosophy.
- `docs/PERMANENT_TERMINAL_SOLUTION.md`: The technical blueprint.
- `cli/src/commands/connect/input.rs`: The current "mess" that needs refactoring.
- `temp/ref/bssh/`: Reference project using the same tech stack (`russh`).

Good luck. The goal is a professional, flicker-free, cross-platform terminal.
