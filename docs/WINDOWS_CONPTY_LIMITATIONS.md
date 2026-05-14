# The Windows ConPTY Limitation: A Technical Truth

This document outlines the fundamental architectural conflict between Irosh's UX Guidelines ("True Transparency") and the design of the Windows Console Pseudoterminal (ConPTY).

After weeks of attempting to engineer a flawless, cross-platform terminal experience, we have determined that achieving 100% visual perfection when connecting to a **Windows Server** is mathematically impossible without violating our core design principles.

## The Core Conflict

Irosh is designed to be a "True Transparency" client. When a user runs a local command (like `~ls`), the output is printed directly into the active terminal stream. This preserves the user's scrollback history and context (as defined in `UX_GUIDELINES.md`).

However, this design fundamentally clashes with how Windows servers handle remote terminals.

### Linux PTYs (Stream-Based)
When an Irosh client connects to a Linux server, the remote shell (`bash` or `zsh`) operates as a simple stream. It prints text and relative cursor movements. If the Irosh client pauses the stream, prints 50 lines of local output (scrolling the physical terminal), and hands control back to Linux, the Linux shell doesn't care. It simply prints the next prompt wherever the cursor currently is. **The integration is flawless.**

### Windows ConPTY (Stateful Grid)
When an Irosh client connects to a Windows server, the remote shell is wrapped in `ConPTY`. Unlike Linux, ConPTY acts as a headless terminal emulator. It maintains an absolute, stateful 2D grid of the screen in the server's memory.
When it draws a prompt, it uses **absolute coordinates** (e.g., "Draw at Row 24, Column 1").

## The "Floating Prompt" Corruption

When a user executes a local command (`~ls`) against a Windows Server:
1. Irosh prints the local output to the client terminal.
2. The physical terminal scrolls down 50 lines to accommodate the output.
3. **ConPTY does not know the terminal scrolled.** Microsoft provides no API for a client to inform ConPTY of out-of-band screen scrolling.
4. When control is returned to the remote shell, ConPTY attempts to draw the next prompt. It instructs the terminal to jump to absolute `Row 24` (which was the bottom of the screen before the scroll).
5. Because the physical terminal scrolled, `Row 24` is now physically floating in the middle (or top) of the user's viewport.

The result is severe visual corruption: **The current prompt magically appears overlaid on top of past command output in the center of the screen.**

## The Rejected Alternative

The only technical way to prevent ConPTY from desynchronizing is to ensure the main terminal screen *never scrolls* during local commands.
This requires wrapping all local commands in the **Alternate Screen Buffer** (the isolated UI mode used by `nano` or `vim`).

While this completely prevents ConPTY corruption, it was strictly rejected by the project architect. Using the Alternate Screen Buffer violates the `UX_GUIDELINES.md` directive of **Scrollback Integrity**. When the Alternate Screen Buffer closes, the output vanishes, destroying the permanent record of the local command.

## The Final Verdict

We have chosen to prioritize **Scrollback Integrity** over Windows ConPTY visual perfection. 

**This is an accepted limitation of the software:**
If a user connects to a Windows server and runs a local command that causes the screen to scroll, the subsequent remote prompt will suffer from absolute coordinate corruption. The user must manually execute `cls` or `Clear-Host` to resynchronize the ConPTY grid.

*(Note: The official Microsoft OpenSSH client suffers from this exact same architectural limitation when using the `~C` escape sequence against a Windows OpenSSH server).*
