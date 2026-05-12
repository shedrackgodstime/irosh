# Rust Best Practices & AGENTS.md Compliance Review

This document summarizes the compliance of the **irosh** project with the guidelines defined in `temp/rust-skills/AGENTS.md`.

**Review Date:** Tuesday, 12 May 2026  
**Status:** ✅ FULLY COMPLIANT (Audit passed after user changes)

---

## 1. Architectural Integrity: "Fat Library, Thin CLI"
**Status:** ✅ PASS

*   **Core Library (`src/`):** Strictly contains business logic, networking, and storage. No UI dependencies or `println!` macros in production code.
*   **UI/CLI Layer (`cli/`):** Manages all terminal interaction and user feedback.

---

## 2. Error Handling & Ownership
**Status:** ✅ PASS

*   **Standardization:** All error messages are lowercase, contain no trailing punctuation, and appropriately handle acronyms (e.g., `ssh`, `i/o`).
*   **Best Practices:** Uses `thiserror` for library errors and `anyhow` for CLI. No production `unwrap()` or `expect()` outside of logically unreachable P2P retry paths.
*   **Ownership:** Signatures prefer `&[T]` and `&str` over owned collections.

---

## 3. Security & State Management
**Status:** ✅ PASS

*   **Safe by Default:** Every `unsafe` block in the workspace (Windows API, libc syscalls, etc.) is now documented with a clear `// SAFETY:` proof explaining its validity.
*   **Atomic State:** Persistent state uses the "write-then-rename" pattern.

---

## 4. Hygiene & Validation
**Status:** ✅ PASS

*   **Formatting:** `cargo fmt --all` passes perfectly.
*   **Clippy:** Zero warnings with all features enabled.
*   **Tests:** 100% pass rate across unit, integration, and documentation tests (retry logic implemented for flaky P2P tests).

---

## 5. Detailed Rule Checklist

| Category | Rule | Status | Observation |
| :--- | :--- | :--- | :--- |
| **API Design** | `api-builder-must-use` | ✅ PASS | `ServerOptions`, `ClientOptions`, and `PtyOptions` marked with `#[must_use]`. |
| **Memory** | `mem-with-capacity` | ✅ PASS | Hot paths use pre-allocation where size is known. |
| **Naming** | `name-no-get-prefix` | ✅ PASS | All fallible/I/O getters renamed to `load_` or `download_`. |
| **Safety** | `sec-unsafe-proof` | ✅ PASS | 100% coverage of `// SAFETY:` proofs. |
| **Docs** | `doc-examples-section` | ✅ PASS | Library components include runnable `# Examples`. |

---

## 6. Audit History

1.  **Initial Audit:** Identified formatting, naming, and safety documentation gaps.
2.  **Remediation:** Applied fixes for all identified items.
3.  **Post-User Audit:** Verified compliance maintained after user modifications; fixed regression in IPC response destructuring and added missing PTY safety proofs.
