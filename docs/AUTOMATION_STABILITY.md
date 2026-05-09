# Irosh Automation & Stability Blueprint

This document defines the requirements for making the Irosh CLI fully programmable and "AI-Ready." The goal is to move from a human-only interactive tool to a "Fat Library, Programmable CLI" architecture.

## 1. Machine-Readable Output (`--json`)
Every command that displays state, configuration, or status MUST support a global `--json` flag.

### Requirements:
*   **Consistency**: The JSON schema for a specific command must be stable across versions.
*   **Standard Fields**: Include timestamps, success booleans, and specific data objects.
*   **Silent Mode**: When `--json` is active, no "pretty" UI (spinners, colors, headers) should be printed to `stdout`. Logs and errors should stay on `stderr`.

### Priority Commands:
*   `irosh status`
*   `irosh system status`
*   `irosh passwd status`
*   `irosh identity show`
*   `irosh peer list`

---

## 2. Non-Interactive Input Handling
Automation scripts (sh, ps1) and AI agents cannot respond to interactive prompts (dialogs, password popups).

### Requirements:
*   **Environment Variables**: Support `IROSH_PASSWORD` and `IROSH_SECRET` to bypass interactive prompts.
*   **Standard Input (Stdin)**: Commands like `passwd set` should detect if they are in a pipe and read the password from `stdin`.
*   **Force Flags (`-y` / `--yes`)**: Any command requiring "danger confirmation" (e.g., `system uninstall`, `passwd remove`) must support a bypass flag for automated cleanup.

---

## 3. Stealth Mode (Ghost Discovery)
Stealth mode is a P2P security feature that makes a node invisible to unauthorized discovery.

### Logic:
*   **Silent Handshake**: The server silently drops any incoming QUIC/ALPN connection requests that do not include a valid "Secret Knock."
*   **Shared Secret (PSK)**: The client must provide a Pre-Shared Key (provided via `--secret` or `IROSH_SECRET`) during the initial P2P handshake.
*   **No Fingerprinting**: The goal is to ensure the server does not reveal its existence to scanners, making it appear "Offline" to anyone without the secret.

---

## 4. Professional Error Handling
*   **Exit Codes**: Use standard Unix exit codes (0 for success, 1 for general error, 69 for service issues, etc.).
*   **Structured Errors**: In `--json` mode, error messages should be encapsulated in the JSON object with a specific error code.

## 5. Implementation Roadmap
1.  **Global Args**: Add `json: bool` and `yes: bool` to the main `Args` struct in `main.rs`.
2.  **Context Update**: Pass the automation flags through the `CliContext` to all command executors.
3.  **UI Abstraction**: Update the `Ui` module to check `ctx.json` before printing colors or boxes.
4.  **Redirection Logic**: In `passwd.rs`, check if `atty` is false or if a flag is set before calling `Ui::password_set()`.
