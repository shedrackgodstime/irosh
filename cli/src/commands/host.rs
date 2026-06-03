use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::{Server, ServerOptions};

pub async fn exec(
    secret: Option<String>,
    auth_mode: Option<crate::commands::CliAuthMode>,
    authorize: Option<std::path::PathBuf>,
    simple: bool,
    ctx: &CliContext,
) -> Result<()> {
    let state = ctx.server_state()?;

    #[cfg(windows)]
    let _ = irosh::sys::windows::job::assign_current_process_to_job();

    if let Some(auth_path) = authorize {
        let key = irosh::russh::keys::ssh_key::PublicKey::read_openssh_file(&auth_path)?;
        let fingerprint = key
            .fingerprint(irosh::russh::keys::ssh_key::HashAlg::Sha256)
            .to_string();
        irosh::storage::trust::write_authorized_client(&state, &fingerprint, &key)?;
        Ui::success(&format!(
            "Authorized new key from {}: {}",
            auth_path.display(),
            fingerprint
        ));
    }

    // Check if vault is empty to show security notice
    let vault = irosh::storage::load_all_authorized_clients(&state)?;
    let password_set = irosh::storage::load_shadow_file(&state)?.is_some();

    if vault.is_empty() && !password_set && !simple && !ctx.args.json {
        Ui::warn(
            "Vault is empty",
            "The first device to connect will be permanently trusted.\nTip: Run 'irosh passwd set' now to require a password instead.",
        );
    }

    let config = irosh::storage::load_config(&state)?;
    let mut options = ServerOptions::new(state.clone());

    if let Some(mode) = auth_mode {
        options = options.auth_mode(mode.into());
    }

    // Apply global config overrides
    if let Some(secret) = secret.or(config.stealth_secret) {
        options = options.secret(secret);
    }

    if let Some(relay_url) = &config.relay_url {
        let mode = irosh::transport::iroh::parse_relay_mode(relay_url)?;
        options = options.relay_mode(mode, Some(relay_url.clone()));
    }

    let stealth_mode = options.secret_value().is_some();

    let (ready, server) = tokio::select! {
        res = Server::bind(options) => match res {
            Ok(res) => res,
            Err(e) => {
                // Check for Identity Conflict (Double Instance)
                let state_root = ctx.server_state_root()?;
                let daemon_running = irosh::IpcClient::new(&state_root)
                    .send(irosh::IpcCommand::GetStatus)
                    .await
                    .is_ok();

                if daemon_running {
                    if ctx.args.json {
                        crate::output::print_error("Daemon is already running.", "daemon_conflict");
                        return Ok(());
                    }
                    Ui::error(
                        "failed to start: the irosh daemon is already running in the background",
                        Some("run 'irosh system status' to inspect it, or 'irosh wormhole' to pair without stopping the daemon"),
                    );
                    anyhow::bail!("Identity conflict.");
                }
                return Err(e.into());
            }
        },
        _ = tokio::signal::ctrl_c() => {
            if !ctx.args.json {
                Ui::info("Host startup cancelled by user.");
            }
            anyhow::bail!("Cancelled");
        }
    };

    let identity = irosh::storage::load_or_generate_identity(&state).await?;
    let fingerprint = identity
        .ssh_key
        .public_key()
        .fingerprint(irosh::russh::keys::ssh_key::HashAlg::Sha256);

    if ctx.args.json {
        #[derive(serde::Serialize)]
        struct HostIdentityJson {
            endpoint_id: String,
            fingerprint: String,
            ticket: String,
            stealth_mode: bool,
        }
        crate::output::print_success(HostIdentityJson {
            endpoint_id: ready.endpoint_id().to_string(),
            fingerprint: fingerprint.to_string(),
            ticket: ready.ticket.to_string(),
            stealth_mode,
        });
    } else if !simple {
        Ui::p2p("Server is starting...");
        Ui::machine_identity(
            ready.endpoint_id(),
            &fingerprint.to_string(),
            &ready.ticket.to_string(),
            "Hosting",
        );
        if stealth_mode {
            Ui::success(
                "Stealth mode ACTIVE — only clients with the correct shared secret can connect. Wormhole pairing is disabled.",
            );
        }
        Ui::info("Press Ctrl+C to stop the server.");
    } else {
        println!("ID: {}", ready.endpoint_id());
        println!("TICKET: {}", ready.ticket);
        if stealth_mode {
            println!("STEALTH: active");
        }
    }

    let shutdown = server.shutdown_handle();
    tokio::spawn(async move {
        irosh::sys::signals::wait_for_shutdown_signal().await;
        if !crate::output::JSON_MODE.load(std::sync::atomic::Ordering::SeqCst) {
            Ui::info("Shutting down gracefully...");
        }
        shutdown.close().await;
    });

    server.run().await?;

    Ok(())
}
