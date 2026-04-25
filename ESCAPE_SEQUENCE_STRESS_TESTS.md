# Irosh Escape Sequence System — Comprehensive Stress Tests

> **Purpose**: This document defines every edge case, race condition, and protocol interaction that the escape sequence system must handle. These tests validate the robustness of the `~`-based UX before shipping to production.

---

## 📋 Test Suite Overview

**Total Tests**: 80+  
**Test Suites**: 18  
**Priority Levels**: P1 (critical), P2 (important), P3 (nice-to-have)  
**Estimated Runtime**: 4-6 hours manual, 15 minutes automated

---

## 🔴 PRIORITY 1: Critical Path Tests (MUST PASS BEFORE MERGE)

These 8 tests validate the core design. Failure on any = regression, do not merge.

### P1.1: Basic Escape Detection — Happy Path
**Setup**: Connected to remote shell  
**Action**: Type `Enter`, then `~.`  
**Expected**: Session disconnects cleanly  
**Verification**:
```
[shell]$ 
~.
[output] Session closed. Returning to local shell...
[exit code] 0
```
**Why it matters**: Core functionality

---

### P1.2: Mid-Line Escape Passthrough
**Setup**: Remote shell, user in middle of typing  
**Action**: Type `echo ~test` (without pressing Enter), then complete line  
**Expected**: `~` is passed to server, server echoes `echo ~test`, output shows `~test`  
**Verification**:
```
$ echo ~test
~test
```
**Why it matters**: No collision with shell syntax

---

### P1.3: Literal Tilde Escape
**Setup**: Connected to shell  
**Action**: Type `Enter`, then `~~`, then `Enter`  
**Expected**: One tilde sent to server and echoed back  
**Verification**: Remote shell receives single `~`, displays correctly  
**Why it matters**: Users can type tilde when needed

---

### P1.4: Help Output
**Setup**: Connected to shell  
**Action**: Type `Enter`, then `~?`  
**Expected**: Help text displays, shell prompt reappears  
**Verification**:
```
$ 
~?
[help text output]
$ 
```
**Why it matters**: Users can discover escape sequences in-session

---

### P1.5: Escape in Inactive Session
**Setup**: Connected session, but network hiccup (0ms latency increase)  
**Action**: Type `~.`  
**Expected**: Disconnect initiated, timeout on network cleanly  
**Verification**: Session closes within 5 seconds  
**Why it matters**: Works when network is degraded

---

### P1.6: Timeout Fires on No Second Byte
**Setup**: Escape system active  
**Action**: Send `~` byte, then **wait 100ms** with no second byte  
**Expected**: `~` is forwarded to server after 50ms timeout  
**Verification**: Server receives `~`, echoes it back  
**Why it matters**: Prevents hanging on incomplete sequences

---

### P1.7: Empty Prompt After Escape
**Setup**: Remote shell  
**Action**: Type `~?`, verify help, then type a command  
**Expected**: Help displays, prompt reappears, command executes normally  
**Verification**:
```
$ 
~?
[help]
$ echo hello
hello
```
**Why it matters**: No residual state after escape

---

### P1.8: Multiple Rapid Escapes
**Setup**: Connected shell  
**Action**: Type `Enter`, `~?`, immediately `~?` again, then `~.`  
**Expected**: First help displays, second help displays, then disconnect  
**Verification**: No crashes, both help texts visible  
**Why it matters**: Handles rapid user input

---

## 🟠 PRIORITY 2: Important Integration Tests

These 15 tests cover real-world scenarios where escapes interact with actual shell tools.

### P2.1: Escape During Vi Editing
**Setup**: `vi file.txt` open in remote shell  
**Action**: 
1. Enter insert mode
2. Type some text
3. Press `~` (as literal character, not command)
4. Press Escape (vim escape key)
5. Type `~` in normal mode (shouldn't trigger escape)
6. Exit vi

**Expected**: Vi works normally, `~` characters in file, no accidental disconnect  
**Verification**: File contains `~` characters, vi exits cleanly  
**Why it matters**: Most common editor on servers

---

### P2.2: Escape During Nano Editing
**Setup**: `nano file.txt` open  
**Action**: Type some text including `~`, save and exit normally  
**Expected**: File contains `~`, no irosh interference  
**Verification**: Nano works, file saved correctly  
**Why it matters**: Nano is safer text editor

---

### P2.3: Escape During Less Paging
**Setup**: `less /etc/passwd`  
**Action**: Scroll, then press `~` in less (character search)  
**Expected**: Less handles `~` as search, irosh doesn't interfere  
**Verification**: Less continues normally, no disconnect  
**Why it matters**: Common pagination tool

---

### P2.4: Escape During Tmux Session
**Setup**: Tmux session active, shell inside tmux pane  
**Action**: Type `~` in tmux command mode (tmux uses `~` in config)  
**Expected**: Tmux processes `~`, irosh doesn't interfere  
**Verification**: Tmux command executes, no unexpected disconnects  
**Why it matters**: Users often use tmux

---

### P2.5: Escape During Bash History Search
**Setup**: Bash shell with history, Ctrl+R active  
**Action**: 
1. Type Ctrl+R to search history
2. Type `~` in search
3. Press Enter to execute

**Expected**: History search works, `~` found in history, command executes  
**Verification**: Correct history command runs  
**Why it matters**: Bash history search is common

---

### P2.6: Escape During Python REPL
**Setup**: `python3` REPL open  
**Action**: 
1. Type: `s = "~hello"`
2. Press Enter (assignment)
3. Type: `print(s)`
4. Press Enter

**Expected**: Python processes `~` as string literal, not escape  
**Verification**: Output: `~hello`  
**Why it matters**: Developers use Python REPLs

---

### P2.7: Escape During Redis-CLI
**Setup**: `redis-cli` connected  
**Action**: 
1. Type: `SET mykey "~value"`
2. Press Enter

**Expected**: Redis processes command, `~` stored as value  
**Verification**: GET returns `"~value"`  
**Why it matters**: Redis CLI uses `:` as prefix (collision risk if we used `:`)

---

### P2.8: Escape During MySQL Client
**Setup**: `mysql -u root -p` session open  
**Action**: 
1. Type: `SELECT '~' as tilde;`
2. Press Enter

**Expected**: MySQL returns row with `~` value  
**Verification**: Query result shows tilde  
**Why it matters**: Database CLI clients use various prefixes

---

### P2.9: Multi-Byte Paste with Escape Sequence
**Setup**: Connected shell  
**Action**: Clipboard contains: `echo ~test && ls`  
**Paste**: Into shell prompt at start of line  
**Expected**: Entire command pasted, executed, `~` passed to server (not intercepted mid-line)  
**Verification**: Command output shows both echo and ls results  
**Why it matters**: Users paste complex commands frequently

---

### P2.10: Escape Timing — User Delay Between ~ and Second Byte
**Setup**: Connected shell  
**Action**: 
1. Type `~`
2. Wait 20ms (human reaction time)
3. Type `?`

**Expected**: Escape detected, help shows (not timed out)  
**Verification**: Help text visible  
**Why it matters**: Users don't type instantly

---

### P2.11: Escape Timing — System Delay in Second Byte Delivery
**Setup**: Connected shell, simulate network latency  
**Action**: 
1. Type `~`
2. Simulate 70ms latency for next byte
3. Type `?`

**Expected**: First byte times out, `~` sent to server, `?` sent to server  
**Verification**: Server echoes `~?`, not help  
**Why it matters**: Network delays happen

---

### P2.12: Progress Bar During Escape Detection
**Setup**: `rsync -v` or `tar` with progress  
**Action**: 
1. Start long transfer
2. Wait for progress bar (uses `\r` only, no `\n`)
3. Type `~?` during progress

**Expected**: Progress bar continues, help not shown (because still mid-line)  
**Verification**: Progress bar visible, escape not triggered  
**Why it matters**: Long operations are common

---

### P2.13: Long Command Line (4000+ chars)
**Setup**: Shell prompt  
**Action**: 
1. Build 4000-char command
2. Press Enter
3. Wait for prompt
4. Type `~?`

**Expected**: Escape works, help displays despite long pending_line history  
**Verification**: Help output after command completes  
**Why it matters**: Some commands are very long

---

### P2.14: Control Characters in Buffer (Ctrl+C, Ctrl+D, Ctrl+U)
**Setup**: Shell with partial command  
**Action**: 
1. Type partial command: `echo hel`
2. Press Ctrl+U (clear line)
3. Press Enter
4. Type `~?`

**Expected**: Line cleared correctly, escape works  
**Verification**: Help displays after Ctrl+U  
**Why it matters**: Users use line editing shortcuts

---

### P2.15: Escape After Terminal Resize
**Setup**: Connected shell  
**Action**: 
1. Resize terminal window
2. Type `~?`

**Expected**: Window resize handled, escape still works  
**Verification**: Help displays at new window size  
**Why it matters**: Users resize terminals frequently

---

## 🟡 PRIORITY 3: Terminal Protocol Edge Cases

These 12 tests cover terminal-specific protocol sequences that might interfere.

### P3.1: Bracketed Paste Mode — Paste Buffer with Tilde
**Setup**: iTerm2, Kitty, or Windows Terminal with bracketed paste enabled  
**Action**: Clipboard = `echo ~test`  
**Paste**: At shell prompt

**Terminal sends**:
```
\x1b[200~echo ~test\x1b[201~
```

**Expected**: Entire clipboard pasted intact, `~` processed as part of command (not escaped)  
**Verification**: Server receives full command, echoes correctly  
**Why it matters**: Pasting is critical, users paste frequently

---

### P3.2: Bracketed Paste Mode — Paste Buffer Containing Escape Sequences
**Setup**: Bracketed paste enabled  
**Clipboard**: Contains literal escape sequence: `echo $'\x1b[33mYellow\x1b[0m'`  
**Paste**: At shell prompt

**Expected**: Escape sequences in clipboard passed through, not interpreted by irosh  
**Verification**: Server processes pasted text correctly  
**Why it matters**: Users paste output from other apps

---

### P3.3: Mouse Tracking Protocol (SGR/Pixel Mode)
**Setup**: Kitty or Alacritty with mouse tracking enabled  
**Action**: 
1. Click on terminal
2. Mouse sends: `\x1b[<0;10;10Mm` (click at position)
3. Then type `~?`

**Expected**: Mouse event processed, escape still works  
**Verification**: Help displays  
**Why it matters**: Modern terminals support mouse

---

### P3.4: Kitty Keyboard Protocol with Alt+~ 
**Setup**: Kitty terminal with keyboard protocol enabled  
**Action**: Press Alt+~ (Alt key + tilde)

**Kitty sends**: `\x1b[27;3u` (ESC code for Alt+~ in new protocol)

**Expected**: Alt+~ not treated as escape (requires unmodified `~`)  
**Verification**: Command not triggered, sequence passed to server if needed  
**Why it matters**: Modern terminals use new keyboard protocols

---

### P3.5: True Color (24-bit) Escape Sequences in Output
**Setup**: Remote app outputs colored text: `\x1b[38;2;255;0;0mRed\x1b[0m`  
**Action**: 
1. Let output display
2. Type `~?` after

**Expected**: Colors display, escape works after  
**Verification**: Help displays correctly  
**Why it matters**: Modern apps use 24-bit color

---

### P3.6: Hyperlink Protocol (OSC 8)
**Setup**: Remote app outputs hyperlink: `\x1b]8;;http://example.com\x07Link\x1b]8;;\x07`  
**Action**: 
1. Hyperlink displays
2. Type `~?`

**Expected**: Hyperlink rendered, escape works  
**Verification**: Help displays  
**Why it matters**: Terminals support clickable links

---

### P3.7: Text Attributes — Blinking Text
**Setup**: Remote outputs: `\x1b[5mBlinking\x1b[0m`  
**Action**: Text blinks, type `~?`

**Expected**: Blinking renders, escape works  
**Verification**: Help displays after blink  
**Why it matters**: Edge case terminal attributes

---

### P3.8: Sixel Graphics (iTerm2/Kitty)
**Setup**: Remote outputs sixel image (large binary sequence)  
**Action**: Image displays, then type `~?`

**Expected**: Image renders, escape works after  
**Verification**: Help displays  
**Why it matters**: Modern terminals support inline images

---

### P3.9: Unicode Emoji and Multi-Byte Characters
**Setup**: Remote outputs: `Hello 👋 世界 ~test 🚀`  
**Action**: Wait for output, type `~?`

**Expected**: Emoji/multi-byte chars display, escape works  
**Verification**: Help displays, no corruption  
**Why it matters**: Globalization support

---

### P3.10: ANSI SGR Sequences with High Parameters
**Setup**: Remote outputs: `\x1b[38;5;196mRed\x1b[0m` (256-color)  
**Action**: Red text displays, type `~?`

**Expected**: Color renders, escape works  
**Verification**: Help displays  
**Why it matters**: Edge case color codes

---

### P3.11: Cursor Position Reporting (CPR)
**Setup**: App requests cursor position: sends `\x1b[6n`  
**Terminal responds**: `\x1b[24;80R` (row 24, col 80)  
**Action**: After CPR exchange, type `~?`

**Expected**: CPR handled, escape works  
**Verification**: Help displays  
**Why it matters**: Some apps query terminal state

---

### P3.12: Focus Events (iTerm2)
**Setup**: Terminal sends focus event: `\x1b[I` (gained focus)  
**Action**: Then type `~?`

**Expected**: Focus event processed, escape works  
**Verification**: Help displays  
**Why it matters**: Terminal events edge case

---

## 🔵 PRIORITY 4: Concurrent I/O and Race Conditions

These 10 tests validate behavior under timing stress.

### P4.1: Rapid Keystroke Sequence
**Setup**: Connected shell  
**Action**: Type 100 keypresses in <100ms: `~?~?~?...~?`  
**Expected**: Multiple helps display, no crashes or duplicates  
**Verification**: Correct number of help blocks shown  
**Why it matters**: Stress tests input buffering

---

### P4.2: Escape During Large Server Output
**Setup**: `cat /usr/share/dict/words` (large file, 5MB+)  
**Action**: 
1. Command starts streaming
2. After ~500KB output, type `~?`

**Expected**: Server output continues, escape processed, help mixed with output  
**Verification**: Help visible somewhere in output, command still running  
**Why it matters**: Output/input interleaving

---

### P4.3: Multiple Escapes in Flight
**Setup**: Connected shell  
**Action**: 
1. Type: `~`
2. Before second byte, type another: `~`
3. Then type: `?`

**Expected**: First `~` times out, sent to server with second `~`, then `?` sent  
**Verification**: Server echoes `~~?`  
**Why it matters**: Overlapping escape sequences

---

### P4.4: Escape + Server Disconnect Race
**Setup**: Connected session about to be dropped  
**Action**: Type `~.` at exact moment server disconnects  
**Expected**: Graceful close, no panic  
**Verification**: No crash, clean shutdown  
**Why it matters**: Network failure edge case

---

### P4.5: Rapid Window Resize During Escape
**Setup**: Terminal window open  
**Action**: 
1. Type `~`
2. Resize window
3. Type `?`

**Expected**: Resize processed, escape handled, help displays  
**Verification**: Help displays at new window size  
**Why it matters**: Concurrent system events

---

### P4.6: Escape While SIGWINCH Pending
**Setup**: Shell with pending window resize signal  
**Action**: Type `~?`

**Expected**: Signal and escape both processed without corruption  
**Verification**: Help displays, window size correct  
**Why it matters**: Signal handling edge case

---

### P4.7: Escape During Shell Pipeline
**Setup**: `yes | head -1000 | wc`  
**Action**: After ~500ms, type `~?`

**Expected**: Pipeline continues, help displays mid-output  
**Verification**: Help visible in stream, pipeline exits cleanly  
**Why it matters**: Complex shell constructs

---

### P4.8: Escape Buffering Under Low Memory
**Setup**: Simulate low-memory condition  
**Action**: Type `~?`

**Expected**: Escape works, no OOM error  
**Verification**: Help displays, app doesn't crash  
**Why it matters**: Embedded/constrained environments

---

### P4.9: Escape with Pending TCP Retransmit
**Setup**: Simulate TCP packet loss (1% drop)  
**Action**: Type `~.` and disconnect

**Expected**: Disconnect sent, retransmits handled, clean close  
**Verification**: Session closes, no hanging  
**Why it matters**: Real network conditions

---

### P4.10: Escape After Long Connection Idle
**Setup**: Connection open for 10 minutes, no activity  
**Action**: Type `~?`

**Expected**: Escape works despite long idle  
**Verification**: Help displays  
**Why it matters**: Long-lived connections

---

## 🟣 PRIORITY 5: Cross-Platform Tests

These 8 tests validate behavior on different OS/terminal combinations.

### P5.1: Linux + xterm
**Setup**: Linux with xterm, SSH to remote  
**Action**: Type `~?`

**Expected**: Works identically to other terminals  
**Verification**: Help displays  
**Why it matters**: VT100 compatibility

---

### P5.2: macOS + iTerm2
**Setup**: macOS with iTerm2  
**Action**: Type `~?`, then bracket paste with `~`  
**Expected**: Both work correctly  
**Verification**: Help and paste work  
**Why it matters**: Popular macOS terminal

---

### P5.3: macOS + Terminal.app
**Setup**: macOS with Terminal.app  
**Action**: Type `~?`

**Expected**: Works  
**Verification**: Help displays  
**Why it matters**: Apple's default terminal

---

### P5.4: Windows 11 + Windows Terminal
**Setup**: Windows Terminal (modern, supports ANSI)  
**Action**: Type `~?`, test escape  
**Expected**: Works identically  
**Verification**: Help displays  
**Why it matters**: Windows support

---

### P5.5: Windows + PowerShell with irosh via WSL
**Setup**: WSL2 Ubuntu + irosh  
**Action**: Type `~?` from WSL  
**Expected**: Works in WSL environment  
**Verification**: Help displays  
**Why it matters**: WSL is popular dev environment

---

### P5.6: Linux + Alacritty
**Setup**: Alacritty (GPU terminal)  
**Action**: Type `~?`, test mouse protocol  
**Expected**: Works, handles modern protocols  
**Verification**: Help displays  
**Why it matters**: Modern terminal emulator

---

### P5.7: Linux + Kitty
**Setup**: Kitty (modern, multi-protocol)  
**Action**: Type `~?`, test keyboard protocol, bracketed paste  
**Expected**: All work correctly  
**Verification**: Help displays, paste works  
**Why it matters**: Advanced terminal features

---

### P5.8: Remote SSH Chain — SSH → irosh
**Setup**: SSH into remote, then `irosh-client <ticket>`  
**Action**: Type `~?` in irosh session  
**Expected**: Escape works (not confused with SSH escapes)  
**Verification**: Help displays in irosh layer, SSH layer unaffected  
**Why it matters**: Nested sessions  

---

## 🟢 PRIORITY 6: Performance and Stress Tests

These 8 tests validate escape handling doesn't degrade performance.

### P6.1: Latency Baseline — No Escapes
**Setup**: Connected shell  
**Measurement**: Measure latency of 1000 keypresses (no escapes)  
**Expected**: <50ms p99 latency  
**Verification**: Baseline established  
**Why it matters**: Performance baseline

---

### P6.2: Latency With Escapes — 1% Escape Rate
**Setup**: Connected shell  
**Measurement**: 1000 keypresses, 10 are `~.` (1%)  
**Expected**: <50ms p99 latency (same as baseline)  
**Verification**: No latency degradation  
**Why it matters**: Escape handling doesn't slow down normal typing

---

### P6.3: CPU Usage — Escape Timeout Loop
**Setup**: Type `~`, wait 100ms (timeout fires)  
**Measurement**: CPU usage during timeout wait  
**Expected**: <1% CPU (idle wait, not busy loop)  
**Verification**: Uses event-based timeout, not polling  
**Why it matters**: Battery and efficiency

---

### P6.4: Memory — Long Session with Many Escapes
**Setup**: Session runs 1 hour, type `~?` every 10 seconds  
**Measurement**: Memory growth  
**Expected**: <5MB growth total (no leaks)  
**Verification**: Memory stable over time  
**Why it matters**: Long-running sessions

---

### P6.5: Throughput — 100MB File Transfer
**Setup**: `~put 100MB_file`  
**Measurement**: Transfer speed, CPU, latency  
**Expected**: >90% of baseline transfer speed  
**Verification**: Escape system overhead <10%  
**Why it matters**: Large file performance

---

### P6.6: Buffering Efficiency — Many Small Packets
**Setup**: 10,000 small packets from server (1-10 bytes each)  
**Measurement**: Time to process, buffering behavior  
**Expected**: All processed efficiently, no quadratic behavior  
**Verification**: Linear time complexity  
**Why it matters**: High-latency/high-jitter networks

---

### P6.7: Terminal I/O Concurrency
**Setup**: Concurrent stdin read, stdout write, stderr write  
**Measurement**: No deadlocks, no stalls  
**Expected**: All I/O processes concurrently  
**Verification**: Tokio task scheduling works  
**Why it matters**: Async correctness

---

### P6.8: Escape Detection Throughput — 1M Characters
**Setup**: Pipe 1M random bytes through escape detector  
**Measurement**: Time taken  
**Expected**: <1 second  
**Verification**: Processor efficiency  
**Why it matters**: Handles large pastes

---

## 📋 Pre-Implementation Checklist

Before implementation begins:

- [ ] All P1 tests defined and understood
- [ ] Timeout value (50ms) confirmed
- [ ] Phase plan (Phase 1 = 2-byte escapes) agreed
- [ ] Terminal testing environment set up (4+ terminal emulators)
- [ ] Memory profiling tools ready
- [ ] Network simulation tools ready (tc, toxiproxy, etc.)
- [ ] CI/CD configured for automated tests
- [ ] Manual test schedule documented
- [ ] Rollback plan prepared
- [ ] Documentation (in-app + README) drafted

---

## 📊 Test Execution Framework

### Automated (Run on CI/CD)
```bash
cargo test --bin irosh-client --test escape_sequences
# Expected: All P1 + P2 tests pass in <2 minutes
```

### Manual Testing Checklist
```
[ ] Run all P1 tests on at least 2 terminal emulators
[ ] Run all P2 tests on vim, nano, tmux, bash
[ ] Run P3 tests on Kitty, iTerm2, Windows Terminal
[ ] Verify P5 cross-platform tests on all supported platforms
[ ] Run P6 performance tests and verify no regression
```

### Release Criteria
- ✅ All P1 tests passing (automated)
- ✅ All P2 tests passing (manual)
- ✅ P3 tests passing on 3+ terminal emulators (manual)
- ✅ P5 tests passing on 3+ platforms (manual)
- ✅ P6 performance tests showing <10% overhead (automated)
- ✅ Zero crashes, hangs, or data corruption in 8 hours manual testing

---

## Success Metrics

**After implementation**:
- 95%+ P1 test pass rate (automated)
- 90%+ P2 test pass rate (manual)
- 85%+ P3 test pass rate (requires terminal-specific setup)
- Zero production incidents related to escape sequences in first month
- User adoption: 50%+ of new connections use escapes

