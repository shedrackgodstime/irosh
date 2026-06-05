//! Shared UI components for the Irosh CLI.

pub mod feedback;
pub mod messages;
pub mod prompts;
pub mod theme;

pub struct Ui;

impl Ui {
    // Re-export feedback components as static methods
    pub fn success(msg: &str) {
        feedback::success(msg);
    }
    pub fn error(msg: &str, tip: Option<&str>) {
        feedback::error(msg, tip);
    }
    pub fn info(msg: &str) {
        feedback::info(msg);
    }
    pub fn warn(title: &str, msg: &str) {
        feedback::warn(title, msg);
    }
    pub fn security(msg: &str) {
        feedback::security(msg);
    }
    pub fn p2p(msg: &str) {
        feedback::p2p(msg);
    }
    pub fn spinner(msg: &str) -> indicatif::ProgressBar {
        feedback::spinner(msg)
    }

    // Re-export prompt components as static methods
    pub fn danger_confirm(msg: &str, expected: &str) -> bool {
        prompts::danger_confirm(msg, expected)
    }
    pub fn soft_confirm(msg: &str) -> bool {
        prompts::soft_confirm(msg)
    }
    pub fn password_set() -> Option<String> {
        prompts::password_set()
    }
    pub fn password_input(prompt: &str) -> Option<String> {
        prompts::password_input(prompt)
    }
    pub fn select<T: std::fmt::Display>(prompt: &str, items: &[T]) -> Option<usize> {
        prompts::select(prompt, items)
    }
    pub fn input(prompt: &str, default: Option<&str>) -> Option<String> {
        prompts::input(prompt, default)
    }

    pub fn machine_identity(endpoint_id: &str, fingerprint: &str, ticket: &str, label: &str) {
        use console::style;
        eprintln!("\n  Machine Identity ({label})");
        eprintln!("  ----------------------------------------------------");
        eprintln!("  Endpoint ID: {endpoint_id}");
        eprintln!("  Fingerprint: {fingerprint}");
        eprintln!("  Ticket:      {}", style(ticket).cyan().bold());
        eprintln!("  ----------------------------------------------------\n");
    }

    /// Print a blank line to stderr (UI formatting spacer).
    pub fn blank() {
        eprintln!();
    }

    /// Print a decorative horizontal rule to stderr.
    pub fn separator() {
        eprintln!("  {}", "─".repeat(52));
    }

    pub fn header(msg: &str) {
        feedback::header(msg);
    }

    pub fn status(label: &str, value: &str, subtext: Option<&str>) {
        feedback::status(label, value, subtext);
    }

    pub fn session_table(sessions: &[irosh::server::ipc::SessionStatus]) {
        use console::style;
        if sessions.is_empty() {
            return;
        }

        eprintln!("\n  Active Sessions");
        eprintln!(
            "  {:<20} {:<12} {:<12} {:<12}",
            "Peer ID", "Duration", "Received", "Sent"
        );
        eprintln!("  {}", "-".repeat(60));

        for session in sessions {
            let peer = if session.peer_id.len() > 18 {
                format!("{}...", &session.peer_id[..15])
            } else {
                session.peer_id.clone()
            };

            let duration =
                if let Ok(start) = chrono::DateTime::parse_from_rfc3339(&session.started_at) {
                    let now = chrono::Utc::now();
                    let diff = now.signed_duration_since(start.with_timezone(&chrono::Utc));
                    if diff.num_hours() > 0 {
                        format!("{}h{}m", diff.num_hours(), diff.num_minutes() % 60)
                    } else {
                        format!("{}m{}s", diff.num_minutes(), diff.num_seconds() % 60)
                    }
                } else {
                    "unknown".to_string()
                };

            eprintln!(
                "  {:<20} {:<12} {:<12} {:<12}",
                style(peer).dim(),
                duration,
                Self::format_bytes(session.bytes_received),
                Self::format_bytes(session.bytes_sent)
            );
        }
        eprintln!();
    }

    #[allow(clippy::cast_precision_loss, reason = "human-readable byte formatting")]
    fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{bytes} B")
        }
    }
}
