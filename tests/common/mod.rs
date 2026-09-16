//! Shared helpers for integration (end-to-end) tests.

use irosh::{Session, SessionEvent, StateConfig};
use std::time::Duration;

/// Helper to create a temporary state directory for tests.
pub fn temp_state(name: &str) -> StateConfig {
    let mut path = std::env::temp_dir();
    path.push(format!("irosh-integ-{}-{}", name, rand::random::<u32>()));
    StateConfig::new(path)
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("irosh=debug,info")
        .with_test_writer()
        .try_init();
}

/// Waits for `needle` to appear in shell output, panicking if the channel
/// closes first or the budget expires.
///
/// `#[allow(dead_code)]`: each integration-test binary compiles `common` in
/// isolation, and only the session-lifecycle suite calls these helpers.
#[allow(dead_code)]
pub async fn expect_shell_output(session: &mut Session, needle: &str) {
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            match session.next_event().await {
                Ok(Some(SessionEvent::Data(data) | SessionEvent::ExtendedData(data, _))) => {
                    seen.extend_from_slice(&data);
                    if String::from_utf8_lossy(&seen).contains(needle) {
                        break;
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => panic!("channel closed before seeing output {needle:?}"),
            }
        }
    })
    .await
    .expect("timed out waiting for shell output");
}

/// Waits until the shell channel reports closure (Closed event or stream end).
#[allow(dead_code)]
pub async fn expect_shell_closed(session: &mut Session) {
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            match session.next_event().await {
                Ok(Some(SessionEvent::Closed) | None) | Err(_) => break,
                Ok(Some(_)) => {}
            }
        }
    })
    .await
    .expect("timed out waiting for idle channel close");
}
