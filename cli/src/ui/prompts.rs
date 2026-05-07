use super::theme::irosh_theme;
use dialoguer::{Confirm, Input, Password, Select};
use std::io::IsTerminal;

/// [C1] Danger Confirmation Prompt (requires typing a specific word).
pub fn danger_confirm(message: &str, expected: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }

    eprintln!("\n\x1b[1;33m[WARN]\x1b[0m {}", message);
    let prompt = format!("Type '{}' to confirm, or press Ctrl+C to cancel", expected);

    match Input::<String>::with_theme(&irosh_theme())
        .with_prompt(prompt)
        .interact()
    {
        Ok(input) => input == expected,
        Err(_) => false,
    }
}

/// [C2] Soft Confirmation Prompt (y/N).
pub fn soft_confirm(message: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }

    Confirm::with_theme(&irosh_theme())
        .with_prompt(format!("\x1b[1;33m[WARN]\x1b[0m {}", message))
        .default(false)
        .interact()
        .unwrap_or(false)
}

/// [C3] Password Set Prompt (with confirmation).
pub fn password_set() -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }

    Password::with_theme(&irosh_theme())
        .with_prompt("Enter new password")
        .with_confirmation("Confirm password", "Passwords do not match. Try again.")
        .interact()
        .ok()
}

/// [C4] Password Input Prompt (single, hidden).
pub fn password_input(prompt: &str) -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }

    Password::with_theme(&irosh_theme())
        .with_prompt(prompt)
        .interact()
        .ok()
}

/// [C5] Interactive List Selector.
pub fn select<T: std::fmt::Display>(prompt: &str, items: &[T]) -> Option<usize> {
    if !std::io::stdin().is_terminal() || items.is_empty() {
        return None;
    }

    Select::with_theme(&irosh_theme())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact()
        .ok()
}

/// [C6] Text Input Prompt (visible).
#[allow(dead_code)]
pub fn input(prompt: &str, default: Option<&str>) -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return default.map(|s| s.to_string());
    }

    let theme = irosh_theme();
    let mut builder = Input::<String>::with_theme(&theme).with_prompt(prompt);

    if let Some(d) = default {
        builder = builder.default(d.to_string());
    }
    builder.interact().ok()
}
