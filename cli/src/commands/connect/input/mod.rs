//! Input engine for handling local keystrokes, remote sync, and escape sequences.

mod actions;
mod editor;
mod engine;

pub use actions::{EscapeAction, InputMode};
pub use engine::InputEngine;
