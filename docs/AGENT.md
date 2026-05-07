# AGENT.md — AI Engineering Directives for Irosh V2 Migration

This document defines the strict behavioral and architectural rules for any AI agent working on this codebase. We are currently executing a **Clean Rewrite / V2 Migration** based on a fully finalized design surface.

Do not write a single line of code without reading and internalizing these rules.

---

## 1. The Migration Mandate

We are moving from a tangled "Discovery" MVP to a professional, production-grade architecture.

- **Do NOT copy-paste old logic verbatim:** The old codebase mixes UI prompts, argument parsing, and networking. When migrating code, you must actively untangle it.
- **The "Fat Library, Thin CLI" Rule is Absolute:** Consult `docs/ARCHITECTURE_CRATE_SPLIT.md`. The core `irosh` crate must NEVER contain `println!`, `dialoguer`, or any UI components. It takes structs and returns `Result`. The `irosh-cli` crate handles all human interaction.
- **Consult the Blueprint:** Every feature, CLI flag, and UX component has already been designed. You must strictly follow:
  1. `docs/PROJECT_DESIGN.md` (Auth flows and security)
  2. `docs/CLI_COMMAND_TREE.md` (Available commands and scope)
  3. `docs/CLI_UX_COMPONENTS.md` (The 9 standard UI prompts)
  4. `docs/ARCHITECTURE_STATE.md` (Atomic writes and file watching)
  5. `docs/ARCHITECTURE_CROSS_PLATFORM.md` (Sys modules and OS isolation)
  6. `docs/SECURITY_AUDIT.md` (Actionable MVP security checklist)

---

## 2. Foundational Mindset

You are here to produce **correct, maintainable, professional Rust 2024 code**.

- **No Guess Coding:** If you do not know an API, stop and say so. Do not hallucinate methods.
- **No Shortcut Coding:** Do NOT use `unwrap()`, `expect()`, `.ok()`, or `todo!()`. Even in tests, prefer returning `Result` from `#[test]` functions or using `assert_matches!`. If a temporary shortcut is absolutely necessary during drafting, it MUST be tagged with `// TODO(audit): remove before commit`. Any code committed with an unresolved `unwrap()` is a failure.
- **No Assumption Coding:** Verify dependencies and struct definitions. Do not assume what they contain.
- **Code must be Complete:** It must compile, handle edge cases, and be idiomatic.

---

## 3. Strict Error Handling

Errors are first-class citizens.

**Core Library (`irosh` crate):**
- Must use `thiserror`.
- Must define specific, structured error variants (e.g., `ConnectionRefused { peer_id: String }`).
- **FORBIDDEN:** `anyhow`, `Box<dyn Error>`, panics, or formatted string errors.

**CLI Executable (`irosh-cli` crate):**
- May use `anyhow` to catch and format library errors into friendly `[ERR]` messages.

---

## 4. Rust & Dependency Hygiene (rust-skills)

- **The `rust-skills` Rulebook:** This workspace contains a highly curated `rust-skills/` directory with 179 rules. You MUST follow these rules, prioritizing the CRITICAL ones:
  - `own-*` (Ownership & Borrowing)
  - `err-*` (Error Handling)
  - `mem-*` (Memory Optimization)
  - `async-*` (Async/Await)
- **Edition:** Rust 2024. Use modern idioms (`let-else`, precise captures `use<'a>`, stable async traits).
- **Dependencies:** Justify every new dependency. Prefer the standard library or widely-audited crates. Do not add a crate for a single 5-line function.
- **Diagnostics & Logging:** Use the `tracing` and `tracing-subscriber` ecosystem for all operational visibility (spans, events, timeouts). Do NOT invent custom `thread_local!` logging systems, as they break in async contexts.
- **Validation Workflow:** As you migrate and write code, you MUST continuously validate the workspace. Before ending a task, ensure the following commands pass with zero errors:
  - `cargo fmt --all --check`
  - `cargo check --workspace`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`

---

## 5. Security & State Management

- **No `unsafe`:** Unless mathematically proven and documented with a `// SAFETY:` comment.
- **Atomic Writes:** All state modifications to `~/.irosh/` must use the `.tmp` rename pattern defined in `docs/ARCHITECTURE_STATE.md` to prevent race conditions.
- **Secrets:** Keys and passwords must never be logged or placed in `Debug` output.

---

## 6. What to do when unsure

Stop and ask the user if:
1. The design docs contain a contradiction.
2. An old piece of MVP code doesn't cleanly map to the new "Fat Library" architecture.
3. You are about to introduce a new major dependency.
4. You need to handle a complex async cancellation or thread-safety issue.

**When in doubt, ask. Do not guess.**
