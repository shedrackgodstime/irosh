# irosh - AI Agent Guide

Project-specific rules live in:

- `scratch/AGENTS.md` - irosh standards, validation gate, domain invariants, and project-specific constraints.

Shared reusable rules live in:

- `.agent-rules/behavior/agent-standards.md` - cross-project agent behavior.
- `.agent-rules/skills/rust/core/SKILL.md` - generic Rust engineering rules.

Local symlinks:

```text
scratch      -> /home/kristency/knowledge-base/projects/irosh
.agent-rules -> /home/kristency/knowledge-base/agent-rules
```

Load shared rules first, then `scratch/AGENTS.md`.
