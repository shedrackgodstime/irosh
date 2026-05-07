use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::diagnostic;

pub async fn exec(ctx: &CliContext) -> Result<()> {
    let state = &ctx.state;

    println!("\n  \x1b[1;36m🩺 irosh Diagnostics\x1b[0m");
    println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");

    // Security
    println!("  \x1b[1;37m🛡️ Security & Storage\x1b[0m");
    let security = diagnostic::check_security(state);

    if security.root_exists {
        if security.root_loose {
            println!(
                "  \x1b[1;33m⚠\x1b[0m  State Directory: Loose permissions ({:04o})",
                security.root_mode.unwrap_or(0)
            );
            println!(
                "     \x1b[38;5;244mTip: Run 'chmod 700 {}'\x1b[0m",
                security.root_path.display()
            );
        } else {
            println!(
                "  \x1b[1;32m✓\x1b[0m  State Directory: Protected ({:04o})",
                security.root_mode.unwrap_or(0)
            );
        }
    }

    if security.key_exists {
        if security.key_unsafe {
            println!(
                "  \x1b[1;31m✖\x1b[0m  Identity Key:    UNSAFE ({:04o})",
                security.key_mode.unwrap_or(0)
            );
            println!(
                "     \x1b[38;5;244mTip: Run 'chmod 600 {}'\x1b[0m",
                security.key_path.display()
            );
        } else {
            println!(
                "  \x1b[1;32m✓\x1b[0m  Identity Key:    Protected ({:04o})",
                security.key_mode.unwrap_or(0)
            );
        }
    }

    // System
    println!("\n  \x1b[1;37m💻 System Environment\x1b[0m");
    let system = diagnostic::check_system();

    if let Some(v) = system.ssh_version {
        println!("  \x1b[1;32m✓\x1b[0m  SSH Binary:      Found ({})", v);
    } else {
        println!("  \x1b[1;31m✖\x1b[0m  SSH Binary:      NOT FOUND");
        println!("     \x1b[38;5;244m'connect' requires an OpenSSH client.\x1b[0m");
    }

    if system.udp_available {
        println!("  \x1b[1;32m✓\x1b[0m  UDP Socket:      Available");
    } else {
        println!("  \x1b[1;31m✖\x1b[0m  UDP Socket:      BLOCKED or UNAVAILABLE");
        println!("     \x1b[38;5;244mIrosh requires UDP for P2P transport.\x1b[0m");
    }

    // Network
    println!("\n  \x1b[1;37m🌐 P2P Network Health\x1b[0m");
    let config = irosh::storage::load_config(state)?;

    let stealth_status = if config.stealth_secret.is_some() {
        "\x1b[1;32mEnabled\x1b[0m"
    } else {
        "\x1b[2mDisabled\x1b[0m"
    };
    println!("     Stealth Mode:    {}", stealth_status);

    let relay_info = if let Some(url) = &config.relay_url {
        if url == "disabled" {
            "\x1b[1;31mDisabled\x1b[0m"
        } else {
            &format!("\x1b[1;33mCustom\x1b[0m (\x1b[2m{}\x1b[0m)", url)
        }
    } else {
        "\x1b[1;32mDefault\x1b[0m"
    };
    println!("     Relay Service:   {}", relay_info);

    let pb = Ui::spinner("Probing transport layer...");

    match diagnostic::probe_network(state).await {
        Ok(probe) => {
            pb.finish_and_clear();
            println!("  \x1b[1;32m✓\x1b[0m  P2P Endpoint:    Online");

            let nat_color = if probe.has_direct_connectivity() {
                "\x1b[1;32m"
            } else {
                "\x1b[1;33m"
            };
            println!(
                "  \x1b[1;32m✓\x1b[0m  NAT Type:        {}{}\x1b[0m",
                nat_color,
                probe.nat_description()
            );

            if probe.has_relay_connectivity() {
                for relay in &probe.relay_urls {
                    println!(
                        "  \x1b[1;32m✓\x1b[0m  Relay Link:      Connected ({})",
                        relay
                    );
                }
            } else {
                println!("  \x1b[1;31m✖\x1b[0m  Relay Link:      DISCONNECTED");
            }
        }
        Err(e) => {
            pb.finish_and_clear();
            println!("  \x1b[1;31m✖\x1b[0m  P2P Endpoint:    OFFLINE");
            println!("     \x1b[31mError: {:#}\x1b[0m", e);
        }
    }

    println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");
    println!(
        "  \x1b[2mRun '\x1b[0m\x1b[35mirosh system status\x1b[0m\x1b[2m' for active session details.\x1b[0m\n"
    );

    Ok(())
}
