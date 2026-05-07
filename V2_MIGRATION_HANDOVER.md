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
The user is ready to start coding. The recommended first steps for this session are:
1. **Initialize the Crate Split:** Modify `Cargo.toml` to formally separate the core `irosh` library (lib.rs) from the executable `irosh-cli` (main.rs), as detailed in `ARCHITECTURE_CRATE_SPLIT.md`.
2. **Setup the `sys` Module:** Create the `sys::unix` and `sys::windows` structure defined in `ARCHITECTURE_CROSS_PLATFORM.md`.
3. **Begin Core Migration:** Start porting the core networking (Iroh) and Auth/Vault logic from the MVP into the clean library crate. 

**Remember:** Do not attempt to fix the broken Windows MVP code. Migrate the working Linux implementation into the clean V2 structure first, using stub functions for Windows. The user will implement the Windows ConPTY logic later when they have access to a Windows PC.

**Good luck. Follow the blueprint.**
