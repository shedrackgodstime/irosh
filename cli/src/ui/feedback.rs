use crate::output::JSON_MODE;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::Ordering;
use std::time::Duration;

// ── Error & Warning ──────────────────────────────────────────────────────────

/// Prints a structured error line (+ optional tip) to stderr.
///
/// Format:
/// ```text
/// error: <msg>
///   tip: <tip>
/// ```
///
/// Silenced in JSON mode — JSON consumers get errors via `print_error()` in
/// `output.rs` instead.
pub fn error(msg: &str, tip: Option<&str>) {
    if JSON_MODE.load(Ordering::SeqCst) {
        return;
    }
    eprintln!("{}: {}", style("error").red().bold(), msg);
    if let Some(t) = tip {
        eprintln!("  {}: {}", style("tip").yellow(), t);
    }
}

/// Prints a recoverable warning + multi-line body to stderr.
///
/// Format:
/// ```text
/// warning: <title>
///     <body line 1>
///     <body line 2>
/// ```
pub fn warn(title: &str, message: &str) {
    if JSON_MODE.load(Ordering::SeqCst) {
        return;
    }
    eprintln!("{}: {}", style("warning").yellow().bold(), title);
    for line in message.lines() {
        eprintln!("    {}", line);
    }
}

// ── Informational ────────────────────────────────────────────────────────────

/// Green check — operation succeeded.
pub fn success(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{} {}", style("✓").green().bold(), msg);
    }
}

/// Blue info — neutral status line.
pub fn info(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{} {}", style("i").blue().bold(), msg);
    }
}

/// Magenta star — security-relevant notice.
pub fn security(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{} {}", style("*").magenta().bold(), msg);
    }
}

/// Cyan arrow — P2P / network action.
pub fn p2p(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{} {}", style(">").cyan().bold(), msg);
    }
}

/// Bold underlined section header.
pub fn header(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("\n  {}", style(msg).bold().underlined());
    }
}

/// Two-column key: value status line (used inside diagnostic sections).
pub fn status(label: &str, value: &str, subtext: Option<&str>) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        let mut line = format!("  {:<16}: {}", style(label).dim(), style(value).bold());
        if let Some(s) = subtext {
            line.push_str(&format!(" ({})", style(s).dim()));
        }
        eprintln!("{}", line);
    }
}

// ── Progress ─────────────────────────────────────────────────────────────────

/// Animated spinner — hidden automatically in JSON mode.
pub fn spinner(message: &str) -> ProgressBar {
    if JSON_MODE.load(Ordering::SeqCst) {
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("-\\|/")
            .template("{spinner:.cyan} {msg}")
            .expect("Valid template"),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}
