use crate::output::JSON_MODE;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// [C7] Warning Banner (non-blocking).
pub fn warn(title: &str, message: &str) {
    eprintln!("{}: {}", style("warning").yellow().bold(), title);
    for line in message.lines() {
        eprintln!("    {}", line);
    }
}

/// [C8] Spinner / Progress Indicator.
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

/// [C9] Success / Failure Banner.
pub fn success(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{} {}", style("v").green().bold(), msg);
    }
}

pub fn error(msg: &str) {
    eprintln!("{}: {}", style("error").red().bold(), msg);
}

pub fn info(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{} {}", style("i").blue().bold(), msg);
    }
}

pub fn security(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{} {}", style("*").magenta().bold(), msg);
    }
}

pub fn p2p(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{} {}", style(">").cyan().bold(), msg);
    }
}

pub fn header(msg: &str) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("\n  {}", style(msg).bold().underlined());
    }
}

pub fn status(label: &str, value: &str, subtext: Option<&str>) {
    if !JSON_MODE.load(Ordering::SeqCst) {
        let mut line = format!("  {:<16}: {}", style(label).dim(), style(value).bold());
        if let Some(s) = subtext {
            line.push_str(&format!(" ({})", style(s).dim()));
        }
        eprintln!("{}", line);
    }
}
