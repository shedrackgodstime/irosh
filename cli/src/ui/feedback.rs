use console::{Emoji, style};
use indicatif::{ProgressBar, ProgressStyle};
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
    println!("{} {}", style(Emoji("✔", "v")).green().bold(), msg);
}

pub fn error(msg: &str) {
    eprintln!("{}: {}", style("error").red().bold(), msg);
}

pub fn info(msg: &str) {
    eprintln!("{} {}", style(Emoji("ℹ", "i")).blue().bold(), msg);
}

pub fn security(msg: &str) {
    eprintln!("{} {}", style(Emoji("🔒", "*")).magenta().bold(), msg);
}

pub fn p2p(msg: &str) {
    eprintln!("{} {}", style(Emoji("📡", ">")).cyan().bold(), msg);
}
