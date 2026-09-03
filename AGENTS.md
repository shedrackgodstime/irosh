# irosh - AI Agent Guide

Welcome to `irosh`. This repository contains **shippable code, tests, and public docs only**.
All private developer memory, specifications, handoffs, and research live in `scratch/`.

## 1. Operating Model & Read Order

When entering this repository, load context in this priority order:

1. **`scratch/HANDOFF.md`**: Read this FIRST. Contains active state, blockers, and immediate next commands.
2. **`.agent-rules/behavior/agent-standards.md`**: Cross-project AI behavior and safety boundaries.
3. **`.agent-rules/behavior/spec-lifecycle.md`**: Drift defense rules (Code is Ground Truth, keeping specs synced).
4. **`scratch/AGENTS.md`**: irosh domain invariants, Cardinal Rules, and validation gates.
5. **`.agent-rules/skills/rust/core/SKILL.md`**: Generic Rust engineering rules (lazy-load sub-rules from `rules/` as needed).

## 2. Working Inside this Project

You have full access to the real codebase, compiler, tests, and linters.
* **Code is Ground Truth**: If existing scratch documents contradict real code, the code wins. Untangle and clean up outdated scratch documents as you implement.
* **Update Specs on Architecture Changes**: If an implementation detail deviates from `scratch/docs_dev/`, update that document before finishing your task.
* **Session Close Mandate**: Always update the 3-bullet status block at the top of `scratch/HANDOFF.md` (`Current State`, `Blockers / Breakages`, `Immediate Next Action`) so the next session starts without amnesia.

## 3. Cardinal Rules (Never Break)

- **Do NOT commit, push, or tag** unless explicitly requested by the user.
- **Validation gate**: `cargo fmt --all --check` + `cargo clippy --all-features -- -D warnings` + `cargo test --workspace --lib`.
- **Every bug fix MUST include a reproducing test.**
- **Never print, log, or commit secrets.**

## 4. Local Machine Wiring

```text
scratch      -> /home/kristency/knowledge-base/projects/irosh
.agent-rules -> /home/kristency/knowledge-base/agent-rules
```

Both `scratch` and `.agent-rules` are local machine symlinks ignored by Git via `.gitignore`.
