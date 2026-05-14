//! Cross-platform signal handling for graceful shutdown.

/// Waits for a system shutdown signal.
///
/// On Unix, this listens for SIGINT, SIGTERM, and SIGQUIT.
/// On Windows, this listens for Ctrl+C and Ctrl+Break.
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
        use tokio::signal::windows::{ctrl_break, ctrl_c};
        let sigint = ctrl_c();
        let sigbreak = ctrl_break();

        if let (Ok(mut sigint), Ok(mut sigbreak)) = (sigint, sigbreak) {
            // Note: tokio's ctrl_c handler on Windows captures CTRL_C_EVENT and CTRL_BREAK_EVENT.
            // CTRL_CLOSE_EVENT (console-close) is harder to capture without a custom
            // SetConsoleCtrlHandler, but for most CLI usage, Ctrl+C is the primary path.
            tokio::select! {
                _ = sigint.recv() => tracing::info!("Received Ctrl+C, shutting down..."),
                _ = sigbreak.recv() => tracing::info!("Received Ctrl+Break, shutting down..."),
            }
            tokio::spawn(async move {
                tokio::select! {
                    _ = sigint.recv() => {},
                    _ = sigbreak.recv() => {},
                }
                tracing::warn!("Received second termination signal, forcing exit...");
                std::process::exit(1);
            });
        } else {
            // Fallback to basic ctrl_c
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
