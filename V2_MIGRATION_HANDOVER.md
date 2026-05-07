# Irosh V2 Migration Handover

**To the AI Agent starting the next session:**

## Current State
The planning, architecture, and design phase for Irosh V2 is **100% complete**. 
The repository has been thoroughly cleaned of old scratchpads and bloated design proposals.

The user is now ready to begin the **Clean V2 Migration**, moving from the tangled MVP to a professional "Fat Library, Thin CLI" architecture.

## Your Mandate
Before you write a single line of code, you **MUST** read:
1. `docs/AGENT.md` (This contains strict rules about `unwrap()`, dependencies, and the `tracing` crate).
2. The `rust-skills` directory rules for Rust best practices.

## Source of Truth
All architectural decisions are locked and documented in the `docs/` directory. Do not invent new UX patterns or guess the state management logic. Refer to:
- `docs/PROJECT_DESIGN.md`
- `docs/CLI_COMMAND_TREE.md`
- `docs/CLI_UX_COMPONENTS.md`
- `docs/ARCHITECTURE_STATE.md`
- `docs/ARCHITECTURE_CRATE_SPLIT.md`
- `docs/ARCHITECTURE_CROSS_PLATFORM.md`
- `docs/SECURITY_AUDIT.md`

## Immediate Next Steps
The V2 workspace has already been initialized. The messy MVP code has been moved to `src_old/` and `cli_old/`. 

Your job is to look at the logic in `src_old/` and `cli_old/`, untangle it, and port it into the clean `src/` (Fat Library) and `cli/` (Thin CLI) directories.

1. **Setup the `sys` Module:** Finish building the `sys::unix` and `sys::windows` structure defined in `ARCHITECTURE_CROSS_PLATFORM.md` inside `src/lib.rs`.
2. **Begin Core Migration:** Start porting the core networking (Iroh) and Auth/Vault logic from `src_old/` into the clean library crate `src/`.

**Remember:** Do not attempt to fix the broken Windows MVP code. Migrate the working Linux implementation into the clean V2 structure first, using stub functions for Windows. The user will implement the Windows ConPTY logic later when they have access to a Windows PC.

**Good luck. Follow the blueprint.**
