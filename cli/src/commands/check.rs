use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::diagnostic;

pub async fn exec(ctx: &CliContext) -> Result<()> {
    let state = &ctx.state;

    let security = diagnostic::check_security(state);
    let system = diagnostic::check_system();
    let config = irosh::storage::load_config(state)?;

    let stealth_status = if config.stealth_secret.is_some() {
        "Active (ALPN locked — wormhole pairing disabled)"
    } else {
        "Disabled (open ALPN — any irosh client can probe)"
    };

    let relay_info = if let Some(url) = &config.relay_url {
        if url == "disabled" {
            "Disabled".to_string()
        } else {
            format!("Custom ({})", url)
        }
    } else {
        "Default".to_string()
    };

    if ctx.args.json {
        #[derive(serde::Serialize)]
        struct SecurityJson {
            root_exists: bool,
            root_mode: Option<u32>,
            root_loose: bool,
            key_exists: bool,
            key_mode: Option<u32>,
            key_unsafe: bool,
        }
        #[derive(serde::Serialize)]
        struct SystemJson {
            ssh_version: Option<String>,
            udp_available: bool,
        }
        #[derive(serde::Serialize)]
        struct NetworkJson {
            stealth_mode: String,
            relay_service: String,
            p2p_endpoint: &'static str,
            nat_type: Option<String>,
            relay_urls: Vec<String>,
            error: Option<String>,
        }
        #[derive(serde::Serialize)]
        struct DiagnosticJson {
            security: SecurityJson,
            system: SystemJson,
            network: NetworkJson,
        }

        let mut network_json = NetworkJson {
            stealth_mode: stealth_status.to_string(),
            relay_service: relay_info.clone(),
            p2p_endpoint: "offline",
            nat_type: None,
            relay_urls: vec![],
            error: None,
        };

        match diagnostic::probe_network(state).await {
            Ok(probe) => {
                network_json.p2p_endpoint = "online";
                network_json.nat_type = Some(probe.nat_description().to_string());
                network_json.relay_urls = probe.relay_urls;
            }
            Err(e) => {
                network_json.error = Some(e.to_string());
            }
        }

        crate::output::print_success(DiagnosticJson {
            security: SecurityJson {
                root_exists: security.root_exists,
                root_mode: security.root_mode,
                root_loose: security.root_loose,
                key_exists: security.key_exists,
                key_mode: security.key_mode,
                key_unsafe: security.key_unsafe,
            },
            system: SystemJson {
                ssh_version: system.ssh_version.clone(),
                udp_available: system.udp_available,
            },
            network: network_json,
        });
        return Ok(());
    }

    Ui::header("Irosh Diagnostics");

    // Security
    Ui::info("Security & Storage");

    if security.root_exists {
        if security.root_loose {
            Ui::warn(
                "Loose Permissions",
                &format!(
                    "State directory is world-readable ({:04o}).\n      Tip: Run 'chmod 700 {}'",
                    security.root_mode.unwrap_or(0),
                    security.root_path.display()
                ),
            );
        } else {
            Ui::success(&format!(
                "State Directory: Protected ({:04o})",
                security.root_mode.unwrap_or(0)
            ));
        }
    }

    if security.key_exists {
        if security.key_unsafe {
            Ui::error(
                &format!(
                    "Identity Key: UNSAFE ({:04o}) — run 'chmod 600 {}'",
                    security.key_mode.unwrap_or(0),
                    security.key_path.display()
                ),
                None,
            );
        } else {
            Ui::success(&format!(
                "Identity Key: Protected ({:04o})",
                security.key_mode.unwrap_or(0)
            ));
        }
    }

    // System
    println!();
    Ui::info("System Environment");

    if let Some(v) = system.ssh_version {
        Ui::success(&format!("SSH Binary: Found ({})", v));
    } else {
        Ui::error(
            "SSH Binary: not found",
            Some("install openssh-client, then re-run 'irosh check'"),
        );
    }

    if system.udp_available {
        Ui::success("UDP Socket: Available");
    } else {
        Ui::error(
            "UDP Socket: blocked or unavailable",
            Some("irosh requires UDP for P2P transport — check your firewall settings"),
        );
    }

    // Network
    println!();
    Ui::info("P2P Network Health");

    Ui::status("Stealth Mode", stealth_status, None);
    Ui::status("Relay Service", &relay_info, None);

    let pb = Ui::spinner("Probing transport layer...");

    match diagnostic::probe_network(state).await {
        Ok(probe) => {
            pb.finish_with_message("Done");
            Ui::success("P2P Endpoint: Online");
            Ui::status("NAT Type", probe.nat_description(), None);

            if probe.has_relay_connectivity() {
                for relay in &probe.relay_urls {
                    Ui::success(&format!("Relay Link: Connected ({})", relay));
                }
            } else {
                Ui::error("Relay Link: DISCONNECTED", None);
            }
        }
        Err(e) => {
            pb.finish_with_message("Done");
            Ui::error("P2P Endpoint: OFFLINE", None);
            Ui::info(&format!("Error: {:#}", e));
        }
    }

    println!("  ----------------------------------------------------");
    Ui::info("Run 'irosh system status' for active session details.");
    println!();

    Ok(())
}
