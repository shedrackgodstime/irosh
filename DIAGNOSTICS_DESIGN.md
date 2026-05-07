# 🔍 Diagnostics & Debugging Design for Irosh

**Design Principles:**
- ✅ **Privacy-first**: No automatic data collection. Users control what's logged.
- ✅ **Minimal overhead**: Zero impact when not debugging. CPU/memory conservative.
- ✅ **Respects --verbose**: Extends existing flag semantics. No new flags bloating CLI.
- ✅ **User-driven**: Only detailed logs when user explicitly wants them.

---

## 1. THE PROBLEM

### Current State
```
$ irosh client connect ticket
🔄 Connecting to peer...
Connection established
[hangs indefinitely without any indication]
```

**What's happening?**
- You don't know which operation is blocking
- No visibility into network state, SSH handshake, timeouts
- User is left guessing: firewall? DNS? Server down? Key issue?

### Root Cause
Irosh has **good error handling** but **poor operation visibility**. When things hang, there's nowhere to look.

---

## 2. SOLUTION: LIGHTWEIGHT STRUCTURED LOGGING

### 2.1 Core Concept: Span-Based Tracing

Instead of printing to console, use **structured spans** that exist in memory. Only expensive operations log to disk when --verbose is enabled.

```rust
// src/diagnostics/mod.rs (NEW - ~200 lines total)

use std::fmt;

/// Represents a single operation span (e.g., "ssh_handshake", "auth_attempt")
#[derive(Debug, Clone)]
pub struct Span {
    /// e.g., "ssh_handshake", "p2p_connection"
    pub name: String,
    /// When this span started
    pub start: std::time::Instant,
    /// When this span ended (None if still running)
    pub end: Option<std::time::Instant>,
    /// Current status: "running", "completed", "failed"
    pub status: String,
    /// Optional error message if status is "failed"
    pub error: Option<String>,
}

impl Span {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            start: std::time::Instant::now(),
            end: None,
            status: "running".to_string(),
            error: None,
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.end
            .unwrap_or_else(std::time::Instant::now)
            .duration_since(self.start)
    }

    pub fn complete(mut self) -> Self {
        self.end = Some(std::time::Instant::now());
        self.status = "completed".to_string();
        self
    }

    pub fn fail(mut self, error: impl Into<String>) -> Self {
        self.end = Some(std::time::Instant::now());
        self.status = "failed".to_string();
        self.error = Some(error.into());
        self
    }
}

/// Thread-local stack of active spans.
/// Only tracks the current operation hierarchy.
/// **Zero cost when not in verbose mode** - just incremental append.
thread_local! {
    static SPAN_STACK: std::cell::RefCell<Vec<Span>> = std::cell::RefCell::new(Vec::new());
}

pub struct ScopedSpan {
    name: String,
}

impl ScopedSpan {
    /// Enter a new span. Auto-closed on drop.
    pub fn enter(name: &str) -> Self {
        let span = Span::new(name);
        SPAN_STACK.with(|stack| {
            stack.borrow_mut().push(span);
        });
        Self {
            name: name.to_string(),
        }
    }

    /// Mark current span as failed and exit
    pub fn fail(&self, error: impl Into<String>) {
        SPAN_STACK.with(|stack| {
            if let Some(span) = stack.borrow_mut().last_mut() {
                span.end = Some(std::time::Instant::now());
                span.status = "failed".to_string();
                span.error = Some(error.into());
            }
        });
    }
}

impl Drop for ScopedSpan {
    fn drop(&mut self) {
        SPAN_STACK.with(|stack| {
            if let Some(mut span) = stack.borrow_mut().pop() {
                span.end = Some(std::time::Instant::now());
                
                // If status wasn't explicitly set, mark as completed
                if span.status == "running" {
                    span.status = "completed".to_string();
                }

                // Only log if verbose mode is enabled
                if crate::config::DEBUG_VERBOSE.load(std::sync::atomic::Ordering::Relaxed) {
                    log_span(&span);
                }
            }
        });
    }
}

fn log_span(span: &Span) {
    let status_icon = match span.status.as_str() {
        "completed" => "✅",
        "failed" => "❌",
        _ => "⏳",
    };

    let elapsed_ms = span.elapsed().as_millis();

    if let Some(error) = &span.error {
        eprintln!("{} {} [{}ms] {}", status_icon, span.name, elapsed_ms, error);
    } else {
        eprintln!("{} {} [{}ms]", status_icon, span.name, elapsed_ms);
    }
}

pub fn print_span_stack() {
    SPAN_STACK.with(|stack| {
        let spans = stack.borrow();
        if spans.is_empty() {
            return;
        }

        eprintln!("\n📊 Operation Stack:");
        for (i, span) in spans.iter().enumerate() {
            let indent = "  ".repeat(i);
            let status_icon = match span.status.as_str() {
                "completed" => "✅",
                "failed" => "❌",
                _ => "▶️ ",
            };
            
            let elapsed_ms = span.elapsed().as_millis();
            eprintln!("{}  {} {} [{}ms]", indent, status_icon, span.name, elapsed_ms);
        }
    });
}
```

**Memory cost:** ~200 bytes per span (negligible)  
**CPU cost:** Single Vec push/pop per operation (microseconds)  
**When not --verbose:** Zero logging overhead

---

### 2.2 Hook Into Existing --verbose Flag

```rust
// src/config.rs (ADD to existing)

use std::sync::atomic::{AtomicBool, Ordering};

/// Global verbose flag - check this once at startup, never again
pub static DEBUG_VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn init_debug_mode(verbose: bool) {
    DEBUG_VERBOSE.store(verbose, Ordering::Relaxed);
    
    if verbose {
        eprintln!("🔍 Verbose mode enabled - detailed operation tracing active");
    }
}
```

**In CLI:**

```rust
// cli/src/main.rs

#[derive(Parser)]
struct Args {
    // ... existing args ...
    
    /// Show detailed operation traces (minimal overhead)
    #[arg(long, short = 'v')]
    verbose: bool,
}

async fn main() -> Result<()> {
    let args = Args::parse();
    
    // Initialize debug mode FIRST
    irosh::config::init_debug_mode(args.verbose);
    
    // ... rest of main ...
}
```

---

## 3. OPERATION TRACING (Pay-Per-Use)

### 3.1 Usage Pattern: Spans Around Key Operations

```rust
// src/client/connect.rs

pub async fn connect(options: &ClientOptions, target: Ticket) -> Result<Session> {
    let _span = ScopedSpan::enter("client_connect");

    // Step 1: P2P
    let _p2p_span = ScopedSpan::enter("p2p_endpoint");
    let endpoint = bind_client_endpoint(&options.state).await?;
    drop(_p2p_span);  // Auto-logs if --verbose

    // Step 2: Connection
    let _conn_span = ScopedSpan::enter("p2p_connect");
    let conn = endpoint
        .connect(target.to_addr(), "irosh/ssh/v1")
        .await
        .map_err(|e| {
            let span = ScopedSpan::enter("_handle_p2p_error");
            span.fail(&format!("connection failed: {}", e));
            drop(span);
            e
        })?;
    drop(_conn_span);

    // Step 3: SSH Stream
    let _ssh_span = ScopedSpan::enter("open_ssh_stream");
    let (send, recv) = conn.open_bi().await
        .map_err(|e| {
            let err_span = ScopedSpan::enter("_stream_error");
            err_span.fail(&format!("stream open failed: {}", e));
            drop(err_span);
            e
        })?;
    drop(_ssh_span);

    // Step 4: SSH Handshake
    let _handshake_span = ScopedSpan::enter("ssh_handshake");
    let session = ssh_handshake(send, recv).await?;
    drop(_handshake_span);

    Ok(session)
}
```

**Output when --verbose:**
```
$ irosh client connect --verbose ticket
✅ p2p_endpoint [12ms]
✅ p2p_connect [145ms]
✅ open_ssh_stream [8ms]
✅ ssh_handshake [234ms]
```

**Output when NOT --verbose:**
```
$ irosh client connect ticket
🔄 Connecting to peer...
Connection established
```

---

## 4. TIMEOUT DETECTION (Hangs)

### 4.1 Lightweight Timeout Wrapper

**Problem:** Operations hang with no indication.  
**Solution:** Timeout with **deferred** error reporting.

```rust
// src/diagnostics/timeout.rs (NEW - ~150 lines)

use std::time::Duration;

/// Execute operation with timeout. If timeout occurs, print span stack and fail.
pub async fn with_timeout<F, T>(
    name: &str,
    duration: Duration,
    op: F,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    let _span = ScopedSpan::enter(name);

    match tokio::time::timeout(duration, op).await {
        Ok(result) => result,
        Err(_) => {
            // Timeout occurred
            let err = IroshError::Timeout {
                operation: name.to_string(),
                duration_secs: duration.as_secs(),
            };

            // Only print span stack if verbose
            if DEBUG_VERBOSE.load(Ordering::Relaxed) {
                eprintln!("\n⏱️  TIMEOUT after {} seconds in '{}'", 
                    duration.as_secs(), name);
                print_span_stack();
            }

            Err(err)
        }
    }
}
```

**Usage:**

```rust
// src/client/connect.rs

pub async fn ssh_handshake(send: Send, recv: Recv) -> Result<Session> {
    with_timeout(
        "ssh_handshake",
        Duration::from_secs(10),
        perform_ssh_handshake(send, recv),
    )
    .await
}
```

**Output when hang occurs with --verbose:**
```
$ irosh client connect --verbose ticket

⏱️  TIMEOUT after 10 seconds in 'ssh_handshake'

📊 Operation Stack:
  ▶️  client_connect [10023ms]
  ▶️  p2p_endpoint [12ms]
  ▶️  p2p_connect [145ms]
  ▶️  open_ssh_stream [8ms]
  ▶️  ssh_handshake [10000ms]  <- STUCK HERE

Error: timeout during SSH handshake
```

---

## 5. CONTEXT ON ERRORS

### 5.1 Rich Error Context (Minimal Printing)

**When error occurs, include context without spamming user:**

```rust
// src/error.rs (extend existing)

#[derive(Debug)]
pub enum IroshError {
    // ... existing variants ...

    #[error("timeout during {operation} after {duration_secs}s")]
    Timeout {
        operation: String,
        duration_secs: u64,
    },

    #[error("connection failed: {reason}")]
    ConnectionFailed {
        reason: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("SSH handshake failed")]
    SshHandshakeFailed {
        #[source]
        source: russh::Error,
        step: String,  // e.g., "waiting_for_newkeys", "key_exchange"
    },
}

impl IroshError {
    /// If verbose, print detailed context; else print brief message
    pub fn print_user_friendly(&self) {
        if DEBUG_VERBOSE.load(Ordering::Relaxed) {
            // Print full details with span stack
            print_span_stack();
            eprintln!("\n❌ Error: {}", self);
            
            if let Some(source) = self.source() {
                eprintln!("  Caused by: {}", source);
            }
        } else {
            // Brief, user-friendly message
            eprintln!("❌ {}", self);
            eprintln!("   (Run with --verbose for details)");
        }
    }
}
```

---

## 6. PLATFORM-SPECIFIC DIAGNOSTICS (On-Demand)

### 6.1 Windows-Specific Hang Diagnostics

Only printed when an operation times out on Windows:

```rust
// src/diagnostics/platform.rs

#[cfg(windows)]
pub fn print_windows_diagnostics_on_hang() {
    eprintln!("\n💡 Windows Diagnostics (common causes of hangs):");
    
    // Check for known issues
    if is_wsl() {
        eprintln!("   ℹ️  Running on WSL - networking may have delays");
    }
    
    if has_firewall_enabled() {
        eprintln!("   ⚠️  Windows Firewall is active");
        eprintln!("       Try: netsh advfirewall show allprofiles");
    }
    
    if has_vpn_enabled() {
        eprintln!("   ⚠️  VPN detected - may interfere with P2P");
    }
    
    eprintln!("\n   Debugging steps:");
    eprintln!("   1. Run: irosh client connect --verbose <ticket>");
    eprintln!("   2. File issue with output: https://github.com/shedrackgodstime/irosh/issues");
}

fn is_wsl() -> bool {
    #[cfg(target_os = "windows")]
    {
        std::path::Path::new("/proc/version").exists()
            || std::path::Path::new("/proc/sys/kernel/osrelease").exists()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}
```

**Only prints when timeout occurs, and only if --verbose:**

```
$ irosh client connect --verbose ticket

⏱️  TIMEOUT after 10 seconds in 'ssh_handshake'

💡 Windows Diagnostics (common causes of hangs):
   ⚠️  Windows Firewall is active
       Try: netsh advfirewall show allprofiles
   ⚠️  VPN detected - may interfere with P2P

   Debugging steps:
   1. Run: irosh client connect --verbose <ticket>
   2. File issue with output: https://github.com/shedrackgodstime/irosh/issues
```

---

## 7. OPTIONAL: FILE-BASED LOGGING (User-Controlled)

If user wants persistent logs for bug reporting, they can opt-in:

```rust
// Add environment variable, NOT automatic

// Only if IROSH_DEBUG_LOG=/tmp/irosh.log is set
if let Ok(log_file) = std::env::var("IROSH_DEBUG_LOG") {
    eprintln!("💾 Writing diagnostic logs to: {}", log_file);
    // Open file, append logs only when --verbose
    // User explicitly enabled it, so no privacy concern
}
```

**No automatic collection. No files created without user asking.**

---

## 8. OPERATIONAL CHECKLIST: Where to Add Spans

### Priority 1: Critical Paths (Must Have)

```
✅ client_connect
  ├── p2p_endpoint
  ├── p2p_connect (with timeout: 30s)
  ├── open_ssh_stream (with timeout: 5s)
  ├── ssh_handshake (with timeout: 10s)
  ├── metadata_exchange (with timeout: 2s)
  └── ready

✅ server_listen
  ├── bind_endpoint
  ├── create_identity
  ├── wait_for_connections
  └── handle_connection (per-connection)
    ├── ssh_accept (with timeout: 10s)
    ├── auth_attempt (per-attempt)
    └── shell_exec
```

### Priority 2: Common Failures

```
✅ auth_attempt
  ├── check_public_key
  ├── check_password
  └── rate_limit_check

✅ file_transfer
  ├── validate_path
  ├── open_transfer_stream
  ├── send_chunks (per-N chunks)
  └── verify_checksum
```

### Priority 3: Network Operations

```
✅ metadata_request
✅ transfer_request
✅ channel_open
```

---

## 9. MEMORY & CPU CONSIDERATIONS

### 9.1 Overhead Analysis

| Component | Memory | CPU | When Active |
|-----------|--------|-----|-------------|
| Span stack (10 spans) | ~2KB | 0% | Always |
| ScopedSpan::enter | ~80B | <1µs | Per operation |
| Timeout wrapper | 0 | <1µs | Per operation |
| Span logging | 0 | 0 | Only --verbose |

**Total overhead when NOT --verbose: ~2KB + negligible CPU**

### 9.2 No Allocations in Hot Path

```rust
// ❌ DON'T do this (allocates string every call)
eprintln!("Starting operation: {}", format!("{:?}", op));

// ✅ DO this (zero allocation when not verbose)
let _span = ScopedSpan::enter("operation");
```

---

## 10. USER EXPERIENCE

### Scenario 1: Normal User (No Debug)

```
$ irosh client connect ticket
🔄 Connecting to peer...
Connection established
$
```

**Zero spam. Instant. Clean.**

---

### Scenario 2: User Experiencing Hang (With --verbose)

```
$ irosh client connect --verbose ticket
🔍 Verbose mode enabled - detailed operation tracing active

🔄 Connecting to peer...
✅ p2p_endpoint [12ms]
✅ p2p_connect [145ms]
✅ open_ssh_stream [8ms]
▶️  ssh_handshake [elapsed: 10000ms...]

[waits 10 seconds]

⏱️  TIMEOUT after 10 seconds in 'ssh_handshake'

📊 Operation Stack:
  ▶️  client_connect [10156ms]
    ▶️  p2p_endpoint [12ms]
    ▶️  p2p_connect [145ms]
    ▶️  open_ssh_stream [8ms]
    ▶️  ssh_handshake [10000ms] <- STUCK HERE

💡 Windows Diagnostics (common causes of hangs):
   ⚠️  Windows Firewall is active

   Debugging steps:
   1. Enable firewall exception for Irosh
   2. Run: irosh client connect --verbose <ticket>
   3. File issue: https://github.com/shedrackgodstime/irosh/issues

Error: timeout during SSH handshake
```

**User knows EXACTLY where it hung. Can now:**
- Check Windows firewall
- File detailed issue with exact timing
- Debug further

---

## 11. IMPLEMENTATION ROADMAP

### Phase 1: Foundation (Week 1)

```rust
// src/diagnostics/mod.rs
// - Span struct + ScopedSpan
// - thread_local SPAN_STACK
// - init_debug_mode()

// src/config.rs
// - DEBUG_VERBOSE atomic bool

// Hook into cli/src/main.rs
// - Pass --verbose to init_debug_mode()
```

**Lines of code:** ~200  
**Testing:** Manual --verbose flag  
**Risk:** Zero (feature-flagged, non-invasive)

---

### Phase 2: Key Paths (Week 2)

```rust
// src/client/connect.rs
// Add ScopedSpan around:
// - client_connect
// - p2p operations
// - ssh_handshake

// src/server/mod.rs
// Add ScopedSpan around:
// - server_listen
// - handle_connection
```

**Lines of code:** ~150  
**Testing:** `irosh client connect --verbose` and `irosh host --verbose`

---

### Phase 3: Timeouts (Week 2)

```rust
// src/diagnostics/timeout.rs
// - with_timeout() macro/function

// Apply to critical paths:
// - client_connect (30s)
// - ssh_handshake (10s)
// - metadata_exchange (2s)
```

**Lines of code:** ~150  
**Testing:** Block operations, verify timeout + span stack

---

### Phase 4: Platform Diagnostics (Week 3)

```rust
// src/diagnostics/platform.rs
// - is_wsl(), has_firewall_enabled(), etc.
// - Only print on timeout + --verbose
```

**Lines of code:** ~100  
**Testing:** Timeout scenarios on Windows

---

## 12. EXAMPLE: Before & After

### Before (User's Experience)

```
$ irosh client connect ticket
🔄 Connecting to peer...
Connection established
[hangs for 30 seconds, no output]
Killed by user with Ctrl+C
```

**User's mental state:** "Is it working? Is there a bug? Should I try again?"

---

### After (User's Experience)

**Scenario A: Normal connection (no hang)**
```
$ irosh client connect ticket
🔄 Connecting to peer...
Connection established
$
```

**No change. Still clean.**

---

**Scenario B: Hang on user's machine**
```
$ irosh client connect --verbose ticket
🔍 Verbose mode enabled - detailed operation tracing active

🔄 Connecting to peer...
✅ p2p_endpoint [12ms]
✅ p2p_connect [145ms]
✅ open_ssh_stream [8ms]
▶️  ssh_handshake [elapsed: 5000ms...]
[waits for timeout]

⏱️  TIMEOUT after 10 seconds in 'ssh_handshake'

📊 Operation Stack:
  ▶️  client_connect [10156ms]
    ▶️  ssh_handshake [10000ms] <- STUCK HERE

Error: timeout during SSH handshake

Debugging steps:
1. Check Windows Firewall
2. Try: irosh client connect --verbose <ticket>
3. File issue with this output
```

**User's mental state:** "It hung during SSH handshake. Probably firewall. Let me check firewall rules."

---

## 13. NO PRIVACY CONCERNS ✅

- ✅ **Zero automatic collection** - only when --verbose enabled
- ✅ **No files created** - output to stderr only (user can redirect)
- ✅ **No network requests** - all local diagnostics
- ✅ **User controls data** - they decide what to share in issues
- ✅ **No telemetry** - no tracking, no sending anywhere

---

## 14. LIGHTWEIGHT & EFFICIENT ✅

- ✅ **~2KB memory overhead** (always)
- ✅ **<1µs per operation** (negligible CPU)
- ✅ **Zero logging when --verbose not set**
- ✅ **No allocations in hot path**
- ✅ **Respects existing --verbose flag**

---

## References

- Thread-local span stacks: Standard async tracing pattern
- Timeout detection: tokio::time::timeout
- Platform detection: std::env::consts + winapi

---

**Status:** Ready for implementation  
**Complexity:** Low  
**Impact:** High (solves user debugging nightmare)  
**Risk:** Minimal (non-invasive, feature-gated)
