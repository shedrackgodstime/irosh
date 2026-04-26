use crate::Args;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use irosh::{StateConfig, diagnostic};
use std::time::Duration;

pub async fn exec(args: &Args) -> Result<()> {
    // Resolve client state directory.
    let state_root = args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    println!();
    println!("\x1b[1mirosh Diagnostics\x1b[0m");
    println!("\x1b[2m{}\x1b[0m", "─".repeat(54));

    // ── Security Checks (no network, instant) ──────────────────────────────────
    println!("\x1b[1mSecurity\x1b[0m");
    check_directory_permissions(&state);
    check_key_permissions(&state);
    println!();

    // ── System Checks ──────────────────────────────────────────────────────────
    println!("\x1b[1mSystem\x1b[0m");
    check_ssh_available();
    println!();

    // ── Network (binds transient endpoint ~2s) ─────────────────────────────────
    println!("\x1b[1mNetwork\x1b[0m");

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
            .template("{spinner:.cyan} {msg}")?,
    );
    pb.set_message("Probing P2P network environment...");
    pb.enable_steady_tick(Duration::from_millis(100));

    match diagnostic::probe_network(&state).await {
        Ok(probe) => {
            pb.finish_and_clear();
            println!("  \x1b[32m[OK]\x1b[0m  P2P Endpoint: Online");

            // NAT Type
            if probe.has_direct_connectivity() {
                println!(
                    "  \x1b[32m[OK]\x1b[0m  Direct Addresses: {} found",
                    probe.direct_addresses.len()
                );
                println!("         NAT: \x1b[32m{}\x1b[0m", probe.nat_description());
            } else {
                println!("  \x1b[33m[!!]\x1b[0m  Direct Addresses: none");
                println!("         NAT: \x1b[33m{}\x1b[0m", probe.nat_description());
            }

            // Relay
            if probe.has_relay_connectivity() {
                for relay in &probe.relay_urls {
                    println!("  \x1b[32m[OK]\x1b[0m  Relay: Connected ({})", relay);
                }
            } else {
                println!("  \x1b[31m[!!]\x1b[0m  Relay: No relay URL found.");
                println!("         Check your network or firewall (UDP/QUIC must be allowed).");
            }
        }
        Err(e) => {
            pb.finish_and_clear();
            println!("  \x1b[31m[!!]\x1b[0m  P2P Endpoint: FAILED");
            println!("         Error: {:#}", e);
            println!("         Check your network connection and state directory.");
        }
    }
    println!();

    // ── Summary ────────────────────────────────────────────────────────────────
    println!("\x1b[2m{}\x1b[0m", "─".repeat(54));
    println!(
        "\x1b[2mRun '\x1b[0m\x1b[36mirosh status\x1b[0m\x1b[2m' for a full environment overview.\x1b[0m"
    );
    println!();

    Ok(())
}

/// Checks the permissions of the state directory.
fn check_directory_permissions(state: &StateConfig) {
    let path = state.root();
    if !path.exists() {
        println!(
            "  \x1b[33m[ ? ]\x1b[0m State directory: Not found (\x1b[2m{}\x1b[0m)",
            path.display()
        );
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.mode() & 0o777;
            if mode & 0o077 == 0 {
                println!(
                    "  \x1b[32m[OK]\x1b[0m  State directory permissions ({:04o})",
                    mode
                );
            } else {
                println!(
                    "  \x1b[33m[!!]\x1b[0m  State directory has loose permissions ({:04o})",
                    mode
                );
                println!("         Recommendation: chmod 700 {}", path.display());
            }
        }
    }
}

/// Checks the permissions of the identity key file and reports issues.
fn check_key_permissions(state: &StateConfig) {
    let key_path = state.root().join("keys").join("node.secret");

    if !key_path.exists() {
        println!(
            "  \x1b[33m[ ? ]\x1b[0m Identity key: Not found (\x1b[2m{}\x1b[0m)",
            key_path.display()
        );
        println!("         Run \x1b[36mirosh host\x1b[0m to initialize.");
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&key_path) {
            Ok(meta) => {
                let mode = meta.mode() & 0o777;
                // Check that group and others have no permissions (mode & 0o077 == 0).
                if mode & 0o077 == 0 {
                    println!(
                        "  \x1b[32m[OK]\x1b[0m  Identity key permissions ({:04o})",
                        mode
                    );
                } else {
                    println!(
                        "  \x1b[31m[!!]\x1b[0m  Identity key has unsafe permissions ({:04o})",
                        mode
                    );
                    println!(
                        "         Run: \x1b[36mchmod 600 {}\x1b[0m",
                        key_path.display()
                    );
                }
            }
            Err(e) => {
                println!("  \x1b[31m[!!]\x1b[0m  Could not read key metadata: {}", e);
            }
        }
    }

    #[cfg(not(unix))]
    {
        println!("  \x1b[32m[OK]\x1b[0m  Identity key: Found");
    }
}

/// Checks if the `ssh` binary is available on the system.
fn check_ssh_available() {
    let output = std::process::Command::new("ssh").arg("-V").output();

    match output {
        Ok(out) => {
            let version = String::from_utf8_lossy(&out.stderr)
                .split_whitespace()
                .next()
                .unwrap_or("unknown")
                .to_string();
            println!(
                "  \x1b[32m[OK]\x1b[0m  SSH Binary: Found (\x1b[2m{}\x1b[0m)",
                version
            );
        }
        Err(_) => {
            println!("  \x1b[31m[!!]\x1b[0m  SSH Binary: NOT FOUND");
            println!("         The \x1b[36mconnect\x1b[0m command requires a local SSH client.");
        }
    }
}
