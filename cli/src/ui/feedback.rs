use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// [C7] Warning Banner (non-blocking).
pub fn warn(title: &str, message: &str) {
    eprintln!("warning: {}", title);
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
            .template("{spinner} {msg}")
            .expect("Valid template"),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// [C9] Success / Failure Banner.
pub fn success(msg: &str) {
    println!("{}", msg);
}

pub fn error(msg: &str) {
    eprintln!("error: {}", msg);
}

pub fn info(msg: &str) {
    eprintln!("{}", msg);
}

pub fn security(msg: &str) {
    eprintln!("security: {}", msg);
}

pub fn p2p(msg: &str) {
    eprintln!("{}", msg);
}
