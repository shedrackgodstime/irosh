use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// [C7] Warning Banner (non-blocking).
pub fn warn(title: &str, message: &str) {
    eprintln!("\x1b[1;35m[SEC]\x1b[0m \x1b[1m{}\x1b[0m", title);
    for line in message.lines() {
        eprintln!("      {}", line);
    }
}

/// [C8] Spinner / Progress Indicator.
pub fn spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
            .template("{spinner:.cyan} {msg}")
            .expect("Valid template"),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// [C9] Success / Failure Banner.
pub fn success(msg: &str) {
    println!("\x1b[1;32m[OK]\x1b[0m {}", msg);
}

pub fn error(msg: &str) {
    eprintln!("\x1b[1;31m[ERR]\x1b[0m {}", msg);
}

pub fn info(msg: &str) {
    eprintln!("\x1b[1;34m[INFO]\x1b[0m {}", msg);
}

pub fn security(msg: &str) {
    eprintln!("\x1b[1;35m[SEC]\x1b[0m {}", msg);
}

pub fn p2p(msg: &str) {
    eprintln!("\x1b[1;36m[P2P]\x1b[0m {}", msg);
}
