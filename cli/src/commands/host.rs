use crate::Args as GlobalArgs;
use anyhow::{Context, Result};
use clap::Args;
use irosh::{SecurityConfig, Server, ServerOptions, StateConfig, config::HostKeyPolicy};
use tracing::{info, info_span};

#[derive(Args, Debug, Clone)]
pub struct HostArgs {
    /// Grant access to ANY client (Danger: disables authentication check).
    #[arg(long, help = "Disable client authorization (UNSAFE)")]
    pub insecure: bool,

    /// Stealth mode secret. Only clients knowing this secret can see the server.
    #[arg(long, env = "IROSH_SECRET", value_name = "PASSPHRASE")]
    pub secret: Option<String>,

    /// Authentication backend mode (key, password, combined). Defaults to key.
    #[arg(long, value_name = "MODE", default_value = "key")]
    pub auth_mode: String,

    /// Password to use when auth_mode is set to 'password' or 'combined'.
    #[arg(long, env = "IROSH_PASSWORD", value_name = "PASSWORD")]
    pub auth_password: Option<String>,

    /// Pre-authorize a specific client public key (OpenSSH format).
    #[arg(long, value_name = "KEY")]
    pub authorize: Vec<String>,

    /// Human-friendly output mode (hides technical jargon).
    #[arg(long)]
    pub simple: bool,
}

pub async fn exec(host_args: HostArgs, global_args: &GlobalArgs) -> Result<()> {
    // 2. Resolve state directory.
    let state_root = global_args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("server")))
        .context("could not determine state directory; please provide --state")?;

    let state = StateConfig::new(state_root);

    info!(path = ?state.root(), "Initializing irosh server state");

    let span = info_span!("server_main");
    let _guard = span.enter();

    // 3. Populate Server Options.
    let mut authorized_keys = Vec::new();
    for key_str in host_args.authorize {
        let key = irosh::russh::keys::ssh_key::PublicKey::from_openssh(&key_str)
            .context("failed to parse authorized key")?;
        authorized_keys.push(key);
    }

    let security = SecurityConfig {
        host_key_policy: if host_args.insecure {
            HostKeyPolicy::AcceptAll
        } else {
            HostKeyPolicy::Tofu
        },
    };
    let mut options = ServerOptions::new(state.clone()).security(security);
    if let Some(secret) = host_args.secret {
        options = options.secret(secret);
    }
    options = options.authorized_keys(authorized_keys.clone());

    // 3.5 Configure Authentication Backend.
    let mut auth_mode = host_args.auth_mode.to_lowercase();

    // If a password is provided but mode is still default 'key', 
    // automatically switch to 'password' for convenience.
    if host_args.auth_password.is_some() && auth_mode == "key" {
        auth_mode = "password".to_string();
    }

    match auth_mode.as_str() {
        "password" => {
            let password = host_args
                .auth_password
                .context("The --auth-password flag is required when --auth-mode=password")?;
            options = options.authenticator(irosh::auth::PasswordAuth::new(password));
        }
        "combined" => {
            let password = host_args
                .auth_password
                .context("The --auth-password flag is required when --auth-mode=combined")?;
            let key_auth = irosh::auth::KeyOnlyAuth::new(security, authorized_keys, state.clone());
            let pass_auth = irosh::auth::PasswordAuth::new(password);
            options = options.authenticator(irosh::auth::CombinedAuth::new(key_auth, pass_auth));
        }
        "key" => {}
        _ => {
            anyhow::bail!("Invalid auth mode: {auth_mode}. Valid options: key, password, combined.")
        }
    }


    // 4. Start the irosh server.
    let (ready, server) = Server::bind(options).await?;

    // 5. Print beautiful connection information.
    if host_args.simple {
        print_ready_simple(&ready);
    } else {
        print_ready_default(&ready);
    }

    if !ready.relay_urls().is_empty() {
        info!(relays = ?ready.relay_urls(), "Server connected to Iroh relays");
    }

    let shutdown = server.shutdown_handle();
    let mut server_task = tokio::spawn(async move { server.run().await });

    tokio::select! {
        res = &mut server_task => {
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => eprintln!("🔥 Server failure: {}", e),
                Err(e) => eprintln!("🔥 Server task failed: {}", e),
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\n👋 Shutting down irosh server...");
            shutdown.close().await;

            match server_task.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => eprintln!("🔥 Server failure during shutdown: {}", e),
                Err(e) => eprintln!("🔥 Server task failed during shutdown: {}", e),
            }
        }
    }

    Ok(())
}

fn print_ready_simple(ready: &irosh::ServerReady) {
    println!("\nirosh server ready");
    println!();
    println!("Ticket:");
    println!("{}", ready.ticket());
    println!();
    println!("Next:");
    println!("Run 'irosh <ticket>' on the client machine.");
    println!();
}

fn print_ready_default(ready: &irosh::ServerReady) {
    println!("\nirosh server ready");
    println!();
    println!("Ticket:");
    println!("{}", ready.ticket());
    println!();
    println!("Host key:");
    println!("{}", ready.host_key_openssh());
    println!();
    println!("Node ID:");
    println!("{}", ready.endpoint_id());
    println!();
    println!("Next:");
    println!("Run 'irosh <ticket>' on the client machine.");
    println!("To keep the server running in the background, use 'irosh system install'.");
    println!();
}
