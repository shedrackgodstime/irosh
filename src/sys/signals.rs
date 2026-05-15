//! Cross-platform signal handling for graceful shutdown.

/// Waits for a system shutdown signal.
///
/// On Unix, this listens for SIGINT, SIGTERM, and SIGQUIT.
/// On Windows, this listens for Ctrl+C and Ctrl+Break via `SetConsoleCtrlHandler`,
/// which works correctly even when the process is a child of `cargo run`.
pub async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let sigint = signal(SignalKind::interrupt());
        let sigterm = signal(SignalKind::terminate());
        let sigquit = signal(SignalKind::quit());

        if let (Ok(mut sigint), Ok(mut sigterm), Ok(mut sigquit)) = (sigint, sigterm, sigquit) {
            tokio::select! {
                _ = sigint.recv() => tracing::info!("Received SIGINT (Ctrl+C), shutting down..."),
                _ = sigterm.recv() => tracing::info!("Received SIGTERM (Termination), shutting down..."),
                _ = sigquit.recv() => tracing::info!("Received SIGQUIT (Quit), shutting down..."),
            }
            tokio::spawn(async move {
                tokio::select! {
                    _ = sigint.recv() => {},
                    _ = sigterm.recv() => {},
                    _ = sigquit.recv() => {},
                }
                tracing::warn!("Received second termination signal, forcing exit...");
                std::process::exit(1);
            });
        } else {
            // Fallback to basic ctrl_c if Unix-specific signals fail to install
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("Received Ctrl+C, shutting down...");
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                tracing::warn!("Received second Ctrl+C, forcing exit...");
                std::process::exit(1);
            });
        }
    }

    #[cfg(windows)]
    {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        use tokio::sync::Notify;
        use windows_sys::Win32::System::Console::{
            CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, SetConsoleCtrlHandler,
        };

        // Shared state between the Win32 callback (which runs on a system thread)
        // and the Tokio async task.
        static NOTIFIER: std::sync::OnceLock<Arc<Notify>> = std::sync::OnceLock::new();
        static SECOND_SIGNAL: AtomicBool = AtomicBool::new(false);

        let notify = Arc::new(Notify::new());
        // Store in the global so the Win32 callback can access it.
        // If set() fails the slot was already filled (shouldn't happen in practice).
        let _ = NOTIFIER.set(notify.clone());

        // Win32 console control handler - runs on a dedicated system thread,
        // so we use only atomics and Notify (both are Send + Sync).
        unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> i32 {
            match ctrl_type {
                CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT => {
                    if SECOND_SIGNAL.swap(true, Ordering::SeqCst) {
                        // Second signal - force-exit immediately, no cleanup.
                        std::process::exit(1);
                    }
                    if let Some(n) = NOTIFIER.get() {
                        n.notify_one();
                    }
                    // Return 1 (TRUE) to suppress the default handler, which
                    // would terminate the process immediately without cleanup.
                    1
                }
                _ => 0, // Let the OS default handler run for other events.
            }
        }

        // Register the handler. Safe because ctrl_handler only touches atomics
        // and a Notify, which are both safe to call from any thread.
        let registered = unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), 1) } != 0;

        if registered {
            tracing::debug!("SetConsoleCtrlHandler registered successfully.");
            notify.notified().await;
            tracing::info!("Received shutdown signal (Ctrl+C / Ctrl+Break), shutting down...");
        } else {
            // Fallback: tokio's built-in ctrl_c. Works in most cases but can
            // miss the signal when running as a `cargo run` subprocess.
            tracing::warn!("SetConsoleCtrlHandler failed, falling back to tokio::signal::ctrl_c()");
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("Received Ctrl+C, shutting down...");
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                tracing::warn!("Received second Ctrl+C, forcing exit...");
                std::process::exit(1);
            });
        }
    }
}
