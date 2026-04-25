# Irosh Escape Sequence UX Design — Critical Implementation Analysis

> **Executive Summary**: The proposed `~`-based escape sequence system is **theoretically sound and collision-proof**, but requires **careful implementation** to avoid subtle failures in edge cases, particularly around terminal protocol interactions, newline state tracking, and timeout behavior.

---

## Design Assessment: ✅ VIABLE BUT NOT TRIVIAL

### What's Right About This Design

#### 1. The Core Rule is Brilliant ✅

> "Escape sequences are only recognized immediately after a newline."

**Why this works**:
- 30 years of SSH proof-of-concept
- Single decision point: is `pending_line` empty?
- Collision-proof: `vi ~` mid-line **cannot** trigger escape (pending_line has characters)
- User mental model aligns: "after I press Enter, `~` is special"

**Result**: Zero conflicts with vi, bash, redis-cli, mysql, sed, awk, python REPL, ipython, node REPL.

#### 2. Raw Terminal Echo Flow is Correct ✅

Your explanation of why silent interception works:

```
User types ~
  → client stdin handler
  → NOT forwarded to session.send()
  → server never sees it
  → server never echoes it back
  → nothing written to stdout
  → screen untouched
```

This is **exactly** how SSH does it. The technical foundation is rock-solid.

#### 3. Integration Point is Perfect ✅

The `cli/bin/client/local.rs` file has exactly the pattern needed:
- `process_stdin_chunk()` receives bytes one at a time
- It already has: `if let Some(buffer) = state.local_command.as_mut() { ... continue; }`
- It already has: `session.send(&[byte]).await?`
- Adding escape logic is one more branch before `session.send()`

**No architectural barriers exist.**

---

## Critical Issues That Will Bite You

### ⚠️ ISSUE #1: Bracketed Paste Mode (HIGH SEVERITY)

**What will happen in production**:

```
User has vim open with bracketed paste mode enabled
$ vim file.txt
[user pastes from clipboard: "~test"]

Terminal sends:
  \x1b[200~
  ~test
  \x1b[201~

Your escape handler sees:
  byte 1: ~
  → enters Escaped state
  byte 2: t
  → resolves as ~t escape (unknown, forward both)
  → sends ~ and t to server
  → loses bracket start marker
```

**Cascading failure**: 
- Paste corrupted
- vim's bracketed paste mode confused
- User's clipboard data corrupted in editor
- **In production: user loses work**

**Why it's hard to test**: Only happens with:
- Specific terminal emulators (iTerm2, Kitty, Windows Terminal)
- Specific applications that enable bracketed paste (vim, neovim, emacs)
- Real clipboard operations (hard to automate)

**Fix required**: **Timeout mechanism**
- After `~` is intercepted, wait max 50ms for second byte
- If no second byte arrives, send `~` to server
- Bracketed paste markers (`\x1b[200~`) are sent as a unit, so:
  - `~` arrives
  - Escape state entered
  - Next byte is `\x1b` (from next terminal packet)
  - 50ms later, timeout fires, `~` sent to server
  - `\x1b[200~` arrives intact, vim processes correctly

---

### ⚠️ ISSUE #2: Newline State Tracking is Wrong (MEDIUM SEVERITY)

**Current code (line 407-419 in cli/bin/client.rs)**:

```rust
for &b in &data {
    if b == b'\n' || b == b'\r' {
        pending_line.clear();
    } else {
        pending_line.push(b);
```

**What this tracks**: Remote echoes that reach the screen.

**What you need**: Start-of-line detection for escape interception.

**The gap**:

```
Timeline:
  T1: User types: echo hello\n
  T2: User's input reaches server
  T3: Server PTY echoes the line back (slowly)
  T4: Server sends newline
  T5: pending_line.clear() executes on client
  T6: User presses ~ (at visible prompt on screen)

Between T6 and T2:
  - Screen shows prompt
  - pending_line was cleared at T5
  - Escape detected correctly ✓

But between T1 and T2:
  - User presses ~ after Enter locally
  - pending_line NOT yet cleared (server echo hasn't arrived)
  - Escape NOT detected ✗
```

**In raw terminal mode, this is actually OK** because:
- The user **sees** the server echo before typing more
- The user won't type `~` until they see the prompt
- But there's a small race window where behavior is undefined

**Fix**: Track BOTH:
1. `just_pressed_newline` from user Enter keypress
2. `pending_line.is_empty()` from server echo

Use OR logic: escape detected if:
```
(just_pressed_newline || pending_line.is_empty()) && byte == b'~'
```

---

### ⚠️ ISSUE #3: Progress Bars and `\r`-Only Updates (MEDIUM SEVERITY)

**What happens**:

```
$ rsync -v source/ dest/
sending incremental file list
file1.txt                    100%  (progress bar uses \r only, no \n)
~?
```

**Your code checks**: `pending_line.is_empty()`

**But**: Progress bars use carriage return (`\r`) without newline (`\n`).
Your code at line 410 only clears on `\n` or `\r`:

```rust
if b == b'\n' || b == b'\r' {
    pending_line.clear();
```

**Wait, it DOES clear on `\r`.** So actually...

**The bug is different**: After progress bar, `pending_line` is empty. User presses `~` during progress bar update (user sees it mid-line visually, but pending_line is empty). Escape fires incorrectly.

**In practice**: This is rare because:
- Progress bars typically have `\n` at the end (after final update)
- User usually waits for prompt before typing
- But it **can** happen with aggressive progress bar apps

**Fix**: Define "start of shell line" more carefully:
- Empty pending_line after actual newline (not just `\r`)
- Or: track last byte type, only intercept if last was `\n` (not `\r` alone)

---

### ⚠️ ISSUE #4: The `:` System Will Interfere (HIGH SEVERITY DURING TRANSITION)

**Current code has two systems**:
1. `:` prefix for local commands (lines 332-343)
2. ANSI escape sequence buffering (lines 253-326)

**Interaction problem**:

```
Current flow:
  line 332-335: Check if start-of-line and byte == b':'
  line 336: Enter local command mode
  line 78-329: Buffer and process

Your escape logic goes where?
  Option A: Before line 332 (escape resolved before : checked)
  Option B: After line 332 (: takes priority)
```

**If Option A** (escape checked first):
- `~put myfile` intercepted, processed
- `:put myfile` never reaches local command handler
- `:` system becomes dead code
- During transition, breaks existing user muscle memory

**If Option B** (: checked first):
- `:put myfile` still works (good)
- `~put myfile` reaches session.send()
- Sent to remote server (bad)
- Escape system doesn't work

**You cannot have both.** The design document says "remove `:` system" but the codebase still has it. This is a migration question.

**Fix required**: Define transition plan:
1. **Phase 1 (v0.3)**: Add `~.`, `~?`, `~~`, `~C` escape logic. Keep `:` system. Both work (redundant but safe).
2. **Phase 2 (v0.4)**: Deprecate `:` in docs. Both still work (warning in help text).
3. **Phase 3 (v1.0)**: Remove `:` system. Only `~` works (breaking change).

Or: Commit to removal now and accept breaking change in next version.

---

### ⚠️ ISSUE #5: Escape Sequence Ordering (MEDIUM SEVERITY)

**What if user types**:

```
~C (should open irosh> prompt)
~put (should inline upload)
~get (should inline download)
~? (should show help)
~. (should disconnect)
~~ (should send literal ~)
```

**Your design says**:
> In escape mode, next byte determines action. Unknown escapes forward both bytes.

**But what about multi-byte escapes like `~put myfile`?**

Current `:` system (line 146):
```rust
if current_line.starts_with(":put ") {
  // Tab completion
```

For `~put`, you'd need:
1. `~` triggers escape
2. `p` is second byte → transition to "command buffering mode"
3. Read `u`, `t`, ` ` → buffer them
4. Read until newline → execute

This is **not** a simple two-byte escape. It's a **variable-length command**.

You're effectively reimplementing the `:` system inside the escape handler.

**Fix**: Phase 1 should support only **fixed-length** escapes:
- `~.` (2 bytes)
- `~?` (2 bytes)
- `~~` (2 bytes)
- `~C` (2 bytes)

Add `~put` and `~get` in **Phase 2** as separate work.

---

### ⚠️ ISSUE #6: Terminal Protocol Edge Cases (HIGH SEVERITY)

**Kitty keyboard protocol** (`\x1b[...u` format):

```
User presses Alt+~
Kitty sends: \x1b[27;3u

Your handler sees:
  byte 1: \x1b
  byte 2: [
  → goes to LocalInputState::Bracketed (line 255)
  → continues
  → keeps reading until final byte
  → never sees ~ at position 4
```

**Actually, wait.** Let me reread your code...

```rust
LocalInputState::Escaped => {
    if byte == b'[' || byte == b'O' {
        *state.input_state = LocalInputState::Bracketed;
    } else {
        *state.input_state = LocalInputState::Normal;
    }
    continue;
}
```

This handles ANSI escape sequences (which start with `\x1b[`). But the problem is:

**User types** `~` (our escape character), not `\x1b`.

If user presses `\x1b` directly (rare, but possible in raw mode):
- `\x1b` triggers `LocalInputState::Escaped`
- Next byte (`[`) triggers `LocalInputState::Bracketed`
- Awaits terminator

This is fine. But what if:

**User pastes clipboard containing terminal protocol**:
```
\x1b[5u\x1b[6u (sent as raw bytes in paste)
```

If this arrives while escape handler is active, could corrupt.

**Fix**: Ensure escape resolver times out and doesn't consume terminal control sequences.

---

## What Will Definitely Work

### ✅ Basic escapes at true shell prompt
```
$ 
~.
→ disconnects
```

### ✅ Mid-line escapes are safe
```
$ echo ~hello
→ sent to server
→ no interception
```

### ✅ Tab completion works afterward
```
$ 
~C
irosh> put [tab]
→ completes local paths
```

### ✅ Server never affected
All escape logic is client-side. Server sees nothing.

---

## Implementation Risk Assessment

| Risk | Severity | Likelihood | Impact | Mitigation |
|------|----------|------------|--------|-----------|
| Bracketed paste corruption | HIGH | MEDIUM | Data loss | Timeout mechanism (50ms) |
| Newline tracking race | MEDIUM | LOW | Wrong escape detect | Track both user input and server echo |
| Progress bar false positive | MEDIUM | LOW | Accidental disconnect | Clear only on \n, not \r |
| `:` system interference | HIGH | HIGH | Feature broken during transition | Commit to Phase plan |
| Multi-byte command buffering | MEDIUM | HIGH | Incomplete implementation | Ship Phase 1 (2-byte escapes only) |
| Terminal protocol conflicts | MEDIUM | MEDIUM | Screen corruption | Timeout prevents hangs |
| Long line buffer overflow | LOW | LOW | Crash | Cap at 1024 bytes (already done) |

---

## Recommended Implementation Phases

### Phase 1: Core (Recommended for v0.3)
- `~.` disconnect
- `~?` help
- `~~` literal tilde
- `~C` open irosh> prompt
- **Timeout: 50ms**
- **Keep `:` system as fallback**
- Effort: ~150 lines of new code

### Phase 2: Inline Transfers (v0.4)
- `~put [-r] <local> [remote]`
- `~get [-r] <remote> [local]`
- Requires command buffering state machine
- Effort: ~200 lines of code + refactoring

### Phase 3: Advanced (v1.0)
- Remove `:` system entirely
- Optimize escape latency
- Add terminal protocol awareness
- Effort: ~50 lines

---

## Testing Strategy

### Automated (Unit Tests)
```
test_escape_not_triggered_mid_line()
test_escape_timeout_sends_tilde()
test_bracketed_paste_passthrough()
test_pending_line_tracking()
```

### Manual (Before Release)
1. **vim** session: `~` normal mode operator doesn't trigger
2. **Bracketed paste**: Paste clipboard with `~` + other chars
3. **Progress bars**: `rsync` with `~?` during progress
4. **Rapid typing**: Type `~` quickly followed by other chars
5. **Long lines**: 5000 char line, then `~?`
6. **Help display**: Verify screen state after `~?`

### Integration (CI/CD)
- Run all unit tests on every push
- Manual tests on release candidates
- Cross-platform (Linux, macOS, Windows)

---

## Success Criteria

### Must Have (Before Ship)
- ✅ `~.` disconnects cleanly
- ✅ Mid-line `~` passes through
- ✅ Timeout prevents hanging
- ✅ Help output doesn't corrupt screen
- ✅ Shell prompt recovers after escape

### Should Have
- ✅ Tab completion in `~C` prompt
- ✅ History tracking
- ✅ Works with vi, nano, vim
- ✅ Bracketed paste works

### Nice to Have
- ✅ `~put` inline transfers
- ✅ `~get` inline transfers
- ✅ Colored help output

---

## Comparison: This Design vs. SSH vs. Current `:` System

| Feature | SSH | Proposed `~` | Current `:` |
|---------|-----|-------------|-----------|
| Trigger character | `~` | `~` | `:` |
| Only after newline | ✅ Yes | ✅ Yes | ✅ Yes |
| Collision-free | ✅ Yes (30y) | ✅ Yes (30y precedent) | ❌ No (conflicts) |
| Interactive prompt | ❌ No | ✅ Yes (`~C`) | ✅ Yes |
| File transfers | ❌ No | ✅ Yes (`~put`/`~get`) | ✅ Yes (`:put`/`:get`) |
| Learning curve | High | Moderate | Low |
| Muscle memory | ✅ SSH users | ✅ SSH users | ❌ Unfamiliar |

---

## Open Questions for Design Confirmation

1. **Timeout value**: 50ms or different? (Recommendation: 50ms)
2. **Phase plan**: Ship all features or iterate? (Recommendation: Phase 1 first)
3. **Escape priority**: What if `~C` and `~put` both possible? (Recommendation: Longest match first)
4. **Error messages**: What does `~C` show if no transfers possible? (Recommendation: `irosh>` prompt anyway)
5. **Documentation**: Where does user learn about escapes? (Recommendation: In-app `~?` help + README)

---

## Final Verdict

**This UX design is GOOD and will ship successfully IF:**

1. ✅ Timeout mechanism is implemented (prevents hangs)
2. ✅ Phase 1 ships with only 2-byte escapes (proves concept)
3. ✅ Transition plan is clear (Phase 2/3 milestone dates)
4. ✅ Comprehensive manual testing on real terminals (before release)
5. ✅ SSH users' muscle memory works (primary success metric)

**Timeline estimate**:
- Phase 1: 2-3 weeks (implementation + testing)
- Phase 2: 1-2 weeks (builds on Phase 1)
- Phase 3: 1 week (cleanup)

**Risk**: MEDIUM (well-designed core, tricky edge cases)

**Reward**: HIGH (SSH users adopt immediately, collision-free UX, superior to current `:` system)

