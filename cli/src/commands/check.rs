use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::diagnostic;

pub async fn exec(ctx: &CliContext) -> Result<()> {
    let state = &ctx.state;

    println!("\n  Irosh Diagnostics");
    println!("  ----------------------------------------------------");

    // Security
    println!("  Security & Storage");
    let security = diagnostic::check_security(state);

    if security.root_exists {
        if security.root_loose {
            println!(
                "  [!] State Directory: Loose permissions ({:04o})",
                security.root_mode.unwrap_or(0)
            );
            println!(
                "      Tip: Run 'chmod 700 {}'",
                security.root_path.display()
            );
        } else {
            println!(
                "  [*] State Directory: Protected ({:04o})",
                security.root_mode.unwrap_or(0)
            );
        }
    }

    if security.key_exists {
        if security.key_unsafe {
            println!(
                "  [x] Identity Key:    UNSAFE ({:04o})",
                security.key_mode.unwrap_or(0)
            );
            println!("      Tip: Run 'chmod 600 {}'", security.key_path.display());
        } else {
            println!(
                "  [*] Identity Key:    Protected ({:04o})",
                security.key_mode.unwrap_or(0)
            );
        }
    }

    // System
    println!("\n  System Environment");
    let system = diagnostic::check_system();

    if let Some(v) = system.ssh_version {
        println!("  [*] SSH Binary:      Found ({})", v);
    } else {
        println!("  [x] SSH Binary:      NOT FOUND");
        println!("      'connect' requires an OpenSSH client.");
    }

    if system.udp_available {
        println!("  [*] UDP Socket:      Available");
    } else {
        println!("  [x] UDP Socket:      BLOCKED or UNAVAILABLE");
        println!("      Irosh requires UDP for P2P transport.");
    }

    // Network
    println!("\n  P2P Network Health");
    let config = irosh::storage::load_config(state)?;

    let stealth_status = if config.stealth_secret.is_some() {
        "Enabled"
    } else {
        "Disabled"
    };
    println!("  Stealth Mode:    {}", stealth_status);

    let relay_info = if let Some(url) = &config.relay_url {
        if url == "disabled" {
            "Disabled".to_string()
        } else {
            format!("Custom ({})", url)
        }
    } else {
        "Default".to_string()
    };
    println!("  Relay Service:   {}", relay_info);

    let pb = Ui::spinner("Probing transport layer...");

    match diagnostic::probe_network(state).await {
        Ok(probe) => {
            pb.finish_and_clear();
            println!("  [*] P2P Endpoint:    Online");
            println!("  [*] NAT Type:        {}", probe.nat_description());

            if probe.has_relay_connectivity() {
                for relay in &probe.relay_urls {
                    println!("  [*] Relay Link:      Connected ({})", relay);
                }
            } else {
                println!("  [x] Relay Link:      DISCONNECTED");
            }
        }
        Err(e) => {
            pb.finish_and_clear();
            println!("  [x] P2P Endpoint:    OFFLINE");
            println!("      Error: {:#}", e);
        }
    }

    println!("  ----------------------------------------------------");
    println!("  Run 'irosh system status' for active session details.\n");

    Ok(())
}
