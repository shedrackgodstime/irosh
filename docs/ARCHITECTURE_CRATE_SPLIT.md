# Crate Architecture: Fat Library, Thin CLI

This document defines the separation of concerns between the core `irosh` library
and the `irosh-cli` executable. This architecture ensures the core library can be
published to crates.io and consumed by other projects (GUIs, bots, automation scripts)
without forcing terminal UI dependencies on them.

---

## 1. The Core Library (`irosh` crate)

**The Rule:** The library knows absolutely nothing about the terminal, colors, prompts, or user interaction.

### Responsibilities:
- **Networking**: Managing the Iroh node, endpoints, ALPN protocols (`irosh/1`, `pairing/v1`).
- **Authentication**: Validating tickets, checking passwords, verifying Ed25519 signatures.
- **State Management**: Reading/writing `state.json`, `config.json`, and the Trust Vault keys using atomic writes.
- **System**: Spawning the PTY (pseudoterminal), managing stdin/stdout byte streams.

### What it MUST NOT do:
- Use `println!`, `eprintln!`, or any direct stdout writes (except passing PTY output).
- Use `dialoguer`, `indicatif`, or any prompt/spinner libraries.
- Panic or `exit()`. It must always return a `Result<T, IroshError>`.

---

## 2. The CLI Executable (`cli` crate / `main.rs`)

**The Rule:** The CLI is purely a translator between the Human and the Core Library. It contains zero business logic.

### Responsibilities:
- **Argument Parsing**: Using `clap` to read what the user wants to do.
- **UX & Prompts**: Showing progress spinners, asking for passwords, showing `[y/N]` confirmations.
- **Formatting**: Taking raw data from the library (e.g., a list of trusted keys) and printing it as a beautiful table.
- **Error Handling**: Catching `IroshError` from the library and translating it into a friendly `[ERR] ...` message.

### What it MUST NOT do:
- Directly write to `state.json` or manage keys. It must ask the library to do it.
- Spawn the PTY directly.
- Implement the wormhole Pkarr logic directly.

---

## 3. Communication Example (Connecting to a Server)

1. **CLI**: Parses `irosh connect my-server`.
2. **CLI**: Asks the Library for the stored Ticket for `my-server`.
3. **Library**: Returns the Ticket.
4. **CLI**: Tells the Library: `Client::connect(Ticket)`.
5. **Library**: Attempts connection. Fails because a password is required. Returns `Error::PasswordRequired`.
6. **CLI**: Catches the error. Prompts the user: `[SEC] Server requires a password: ___`.
7. **CLI**: Tells the Library: `Client::connect_with_password(Ticket, "secret123")`.
8. **Library**: Succeeds, establishes PTY, returns a `Session` object.
9. **CLI**: Hands the terminal over to the `Session`.
