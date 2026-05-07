//! Shared UI components for the Irosh CLI.

pub mod feedback;
pub mod prompts;
pub mod theme;

pub struct Ui;

impl Ui {
    // Re-export feedback components as static methods
    pub fn success(msg: &str) {
        feedback::success(msg);
    }
    pub fn error(msg: &str) {
        feedback::error(msg);
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
    #[allow(dead_code)]
    pub fn input(prompt: &str, default: Option<&str>) -> Option<String> {
        prompts::input(prompt, default)
    }
}
