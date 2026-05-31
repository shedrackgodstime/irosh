//! Unix-specific implementations for PTY handling and service management.
//!
//! These modules are only compiled on Unix-like platforms (Linux, macOS).
//! The Windows counterparts live in `super::windows` (unreachable on Unix).

pub mod pty;
pub mod service;
