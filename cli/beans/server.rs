use anyhow::{Context, Result};
use clap::Parser;
use irosh::{SecurityConfig, Server, ServerOptions, StateConfig, config::HostKeyPolicy};
use std::path::PathBuf;
use tracing::{info, info_span};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[cfg(any(target_os = "macos", test))]
const MACOS_LAUNCHD_LABEL: &str = "dev.irosh.server";
#[cfg(any(target_os = "macos", test))]
const MACOS_LAUNCHD_FILE: &str = "dev.irosh.server.plist";

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Start an irosh P2P SSH server",
    long_about = "Launches a secure SSH-over-P2P listener that creates a bridge to this machine. \
                  It automatically performs NAT hole-punching and relaying, provided a connection \
                  ticket that you can share with clients.\n\n\
                  By default, it uses a 'Trust On First Use' policy or an explicit authorized keys list. \
                  To run silently in the background, use the 'service' command."
)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// The directory used for persistent state (identity, trust, keys).
    #[arg(
        short,
        long,
        env = "IROSH_STATE_DIR",
        value_name = "DIR",
        global = true
    )]
    state: Option<PathBuf>,

    /// Grant access to ANY client (Danger: disables authentication check).
    #[arg(long, help = "Disable client authorization (UNSAFE)")]
    insecure: bool,

    /// Stealth mode secret. Only clients knowing this secret can see the server.
    #[arg(long, env = "IROSH_SECRET", value_name = "PASSPHRASE")]
    secret: Option<String>,

    /// Authentication backend mode (key, password, combined). Defaults to key.
    #[arg(long, value_name = "MODE", default_value = "key")]
    auth_mode: String,

    /// Password to use when auth_mode is set to 'password' or 'combined'.
    #[arg(long, env = "IROSH_PASSWORD", value_name = "PASSWORD")]
    auth_password: Option<String>,

    /// Pre-authorize a specific client public key (OpenSSH format).
    #[arg(long, value_name = "KEY")]
    authorize: Vec<String>,

    /// Human-friendly output mode (hides technical jargon).
    #[arg(long)]
    simple: bool,

    /// Print server connection information and exit. Does NOT start the server.
    #[arg(long, visible_alias = "print-identity")]
    identity: bool,

    /// List all authorized client public keys and exit.
    #[arg(long)]
    list: bool,

    /// Alias for --list.
    #[arg(long)]
    clients: bool,

    /// Enable verbose network logging to stderr.
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Commands {
    /// Background service management (install, start, stop, status).
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Show server connection information without starting the listener.
    Info,
}

#[derive(clap::Subcommand, Clone, Debug)]
enum ServiceAction {
    /// Install the server as a background service.
    Install,
    /// Remove the background service.
    Uninstall,
    /// Start the background service.
    Start,
    /// Stop the background service.
    Stop,
    /// Show the background service status.
    Status,
}

#[cfg(target_os = "linux")]
fn installed_service_file() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config/systemd/user/irosh-server.service"))
}

#[cfg(target_os = "macos")]
fn installed_service_file() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("Library/LaunchAgents/dev.irosh.server.plist"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // 1. Initialize professional logging.
    let level = if args.verbose { "info" } else { "error" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    // 2. Resolve state directory.
    let state_root = args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("server")))
        .context("could not determine state directory; please provide --state")?;

    let state = StateConfig::new(state_root);

    // 2.5 Handle Identity interrogation.
    if args.identity {
        let mut options = ServerOptions::new(state.clone()).security(SecurityConfig {
            host_key_policy: if args.insecure {
                HostKeyPolicy::AcceptAll
            } else {
                HostKeyPolicy::Tofu
            },
        });
        if let Some(secret) = args.secret.clone() {
            options = options.secret(secret);
        }
        let ready = irosh::Server::inspect(&options).await?;
        print_identity_snapshot(&ready);
        return Ok(());
    }

    // 2.6 Handle Client listing.
    if args.list || args.clients {
        let keys = irosh::storage::load_all_authorized_clients(&state)?;
        if keys.is_empty() {
            println!("No authorized clients saved.");
        } else {
            println!("AUTHORIZED CLIENT FINGERPRINTS");
            println!("{}", "-".repeat(60));
            for key in keys {
                println!(
                    "{}",
                    key.fingerprint(irosh::russh::keys::ssh_key::HashAlg::Sha256)
                );
            }
        }
        return Ok(());
    }

    info!(path = ?state.root(), "Initializing irosh server state");

    // 1.5 Handle Subcommands.
    if let Some(command) = args.command {
        match command {
            Commands::Service { action } => {
                handle_service(action, &args.state).await?;
                return Ok(());
            }
            Commands::Info => {
                let mut options = ServerOptions::new(state.clone()).security(SecurityConfig {
                    host_key_policy: if args.insecure {
                        HostKeyPolicy::AcceptAll
                    } else {
                        HostKeyPolicy::Tofu
                    },
                });
                if let Some(secret) = args.secret {
                    options = options.secret(secret);
                }
                let ready = Server::inspect(&options).await?;
                print_info_snapshot(&ready);

                // Add a small service hint.
                #[cfg(target_os = "linux")]
                {
                    if let Some(service_file) = installed_service_file() {
                        if service_file.exists() {
                            println!("Background service: installed");
                            println!("Use `irosh-server service status` for more information.");
                        } else {
                            println!("Background service: not installed");
                        }
                    } else {
                        println!("Background service: unknown (home directory unavailable)");
                    }
                    println!();
                }

                #[cfg(target_os = "macos")]
                {
                    if let Some(service_file) = installed_service_file() {
                        if service_file.exists() {
                            println!("Background service: installed");
                            println!("Use `irosh-server service status` for more information.");
                        } else {
                            println!("Background service: not installed");
                        }
                    } else {
                        println!("Background service: unknown (home directory unavailable)");
                    }
                    println!();
                }

                return Ok(());
            }
        }
    }

    let span = info_span!("server_main");
    let _guard = span.enter();

    // 3. Populate Server Options.
    let mut authorized_keys = Vec::new();
    for key_str in args.authorize {
        let key = irosh::russh::keys::ssh_key::PublicKey::from_openssh(&key_str)
            .context("failed to parse authorized key")?;
        authorized_keys.push(key);
    }

    let security = SecurityConfig {
        host_key_policy: if args.insecure {
            HostKeyPolicy::AcceptAll
        } else {
            HostKeyPolicy::Tofu
        },
    };
    let mut options = ServerOptions::new(state.clone()).security(security);
    if let Some(secret) = args.secret {
        options = options.secret(secret);
    }
    options = options.authorized_keys(authorized_keys.clone());

    // 3.5 Configure Authentication Backend.
    let auth_mode = args.auth_mode.to_lowercase();
    match auth_mode.as_str() {
        "password" => {
            let password = args
                .auth_password
                .context("The --auth-password flag is required when --auth-mode=password")?;
            options = options.authenticator(irosh::auth::PasswordAuth::new(password));
        }
        "combined" => {
            let password = args
                .auth_password
                .context("The --auth-password flag is required when --auth-mode=combined")?;
            let key_auth = irosh::auth::KeyOnlyAuth::new(security, authorized_keys, state.clone());
            let pass_auth = irosh::auth::PasswordAuth::new(password);
            options = options.authenticator(irosh::auth::CombinedAuth::new(key_auth, pass_auth));
        }
        "key" => {
            // Default behavior, handled by server internally if no authenticator is provided.
            // But we can explicitly provide the KeyOnlyAuth if we want.
            // Leaving it empty preserves default.
        }
        _ => {
            anyhow::bail!("Invalid auth mode: {auth_mode}. Valid options: key, password, combined.")
        }
    }

    // 4. Start the irosh server.
    let (ready, server) = Server::bind(options).await?;

    // 4. Print beautiful connection information.
    if args.simple {
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

fn print_identity_snapshot(ready: &irosh::ServerReady) {
    println!("irosh server identity");
    println!("This command does not start the server.");
    println!("Run `irosh-server` without `--identity` to accept connections.\n");
    println!("Node ID:");
    println!("{}", ready.endpoint_id());
    println!("\nHost key:");
    println!("{}", ready.host_key_openssh());
    println!("\nTicket:");
    println!("{}", ready.ticket());
}

fn print_info_snapshot(ready: &irosh::ServerReady) {
    println!("irosh server info");
    println!("This command does not start the server.\n");
    println!("Node ID:");
    println!("{}", ready.endpoint_id());
    println!("\nHost key:");
    println!("{}", ready.host_key_openssh());
    println!("\nTicket:");
    println!("{}", ready.ticket());
    println!();
}

fn print_ready_simple(ready: &irosh::ServerReady) {
    println!("\nirosh server ready");
    println!();
    println!("Ticket:");
    println!("{}", ready.ticket());
    println!();
    println!("Next:");
    println!("Run `irosh-client <ticket>` on the client machine.");
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
    println!("Run `irosh-client <ticket>` on the client machine.");
    println!("To keep the server running in the background, use `irosh-server service install`.");
    println!();
}

async fn handle_service(action: ServiceAction, state: &Option<std::path::PathBuf>) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        handle_service_windows(action, state).await
    }

    #[cfg(target_os = "linux")]
    {
        handle_service_linux(action, state).await
    }

    #[cfg(target_os = "macos")]
    {
        handle_service_macos(action, state).await
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Err(anyhow::anyhow!(
            "Service management is only supported on Linux (systemd), macOS (launchd), and Windows (Task Scheduler)"
        ))
    }
}

#[cfg(target_os = "linux")]
async fn handle_service_linux(
    action: ServiceAction,
    state: &Option<std::path::PathBuf>,
) -> Result<()> {
    let exe = std::env::current_exe()?;
    let user_home = dirs::home_dir().context("could not find home directory")?;
    let service_dir = user_home.join(".config/systemd/user");
    let service_file = service_dir.join("irosh-server.service");

    match action {
        ServiceAction::Install => {
            std::fs::create_dir_all(&service_dir)?;

            let state_arg = if let Some(p) = state {
                format!(" --state {}", p.display())
            } else {
                "".to_string()
            };

            let unit = format!(
                r#"[Unit]
Description=irosh P2P SSH Server
After=network.target

[Service]
ExecStart={} {}
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
"#,
                exe.display(),
                state_arg
            );

            std::fs::write(&service_file, unit)?;
            println!("✅ Service unit installed to {}", service_file.display());

            std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status()?;
            std::process::Command::new("systemctl")
                .args(["--user", "enable", "irosh-server"])
                .status()?;
            std::process::Command::new("systemctl")
                .args(["--user", "start", "irosh-server"])
                .status()?;

            println!("🚀 Service enabled and started in the background.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "stop", "irosh-server"])
                .status();
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "irosh-server"])
                .status();
            if service_file.exists() {
                std::fs::remove_file(&service_file)?;
                println!("🗑️ Service uninstalled.");
            }
        }
        ServiceAction::Start => {
            std::process::Command::new("systemctl")
                .args(["--user", "start", "irosh-server"])
                .status()?;
            println!("▶️ Service started.");
        }
        ServiceAction::Stop => {
            std::process::Command::new("systemctl")
                .args(["--user", "stop", "irosh-server"])
                .status()?;
            println!("⏹️ Service stopped.");
        }
        ServiceAction::Status => {
            std::process::Command::new("systemctl")
                .args(["--user", "status", "irosh-server"])
                .status()?;
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
async fn handle_service_windows(
    action: ServiceAction,
    state: &Option<std::path::PathBuf>,
) -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();

    let state_arg = if let Some(p) = state {
        format!("/state \"{}\"", p.display())
    } else {
        String::new()
    };

    let task_name = "irosh-server";

    match action {
        ServiceAction::Install => {
            // Create XML for schtasks
            let task_xml = format!(
                r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <Boot />
    <Logon />
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>true</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{}</Command>
      <Arguments>{}</Arguments>
    </Exec>
  </Actions>
</Task>"#,
                exe_str, state_arg
            );

            let temp_dir = std::env::temp_dir();
            let xml_path = temp_dir.join("irosh-task.xml");

            // Write as UTF-16LE with BOM for Windows compatibility
            let mut file = std::fs::File::create(&xml_path)?;
            use std::io::Write;
            file.write_all(&[0xFF, 0xFE])?; // UTF-16 LE BOM
            for c in task_xml.encode_utf16() {
                let bytes = c.to_le_bytes();
                file.write_all(&bytes)?;
            }

            std::process::Command::new("schtasks")
                .args([
                    "/create",
                    "/tn",
                    task_name,
                    "/xml",
                    &xml_path.display().to_string(),
                    "/f",
                ])
                .status()?;

            std::fs::remove_file(&xml_path)?;

            println!("✅ Windows Task Scheduler task created: {}", task_name);
            println!("🚀 Service will start on next login or boot.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("schtasks")
                .args(["/delete", "/tn", task_name, "/f"])
                .status();
            println!("🗑️ Task Scheduler task removed.");
        }
        ServiceAction::Start => {
            std::process::Command::new("schtasks")
                .args(["/run", "/tn", task_name])
                .status()?;
            println!("▶️ Task started.");
        }
        ServiceAction::Stop => {
            // Windows Task Scheduler doesn't have a direct stop, but we can kill the process
            std::process::Command::new("taskkill")
                .args(["/IM", "irosh-server.exe", "/F"])
                .status()?;
            println!("⏹️ Task stopped.");
        }
        ServiceAction::Status => {
            let output = std::process::Command::new("schtasks")
                .args(["/query", "/tn", task_name])
                .output()?;

            if output.status.success() {
                println!("✅ Task '{}' exists:", task_name);
                println!("{}", String::from_utf8_lossy(&output.stdout));
            } else {
                println!("❌ Task '{}' not found.", task_name);
            }
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn handle_service_macos(
    action: ServiceAction,
    state: &Option<std::path::PathBuf>,
) -> Result<()> {
    let exe = std::env::current_exe()?;
    let user_home = dirs::home_dir().context("could not find home directory")?;
    let service_dir = user_home.join("Library/LaunchAgents");
    let service_file = service_dir.join(MACOS_LAUNCHD_FILE);
    let label = MACOS_LAUNCHD_LABEL;
    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    let service_target = format!("{domain}/{label}");

    match action {
        ServiceAction::Install => {
            std::fs::create_dir_all(&service_dir)?;
            std::fs::create_dir_all(user_home.join("Library/Logs"))?;
            let plist = build_launchd_plist(&exe, state, &user_home);
            std::fs::write(&service_file, plist)?;

            let _ = std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status();
            let _ = std::process::Command::new("launchctl")
                .args(["bootout", &domain, &service_file.display().to_string()])
                .status();

            std::process::Command::new("launchctl")
                .args(["bootstrap", &domain, &service_file.display().to_string()])
                .status()?;

            println!("✅ LaunchAgent installed to {}", service_file.display());
            println!("🚀 Service loaded and started with launchd.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status();
            let _ = std::process::Command::new("launchctl")
                .args(["bootout", &domain, &service_file.display().to_string()])
                .status();
            if service_file.exists() {
                std::fs::remove_file(&service_file)?;
                println!("🗑️ LaunchAgent removed.");
            }
        }
        ServiceAction::Start => {
            if !service_file.exists() {
                return Err(anyhow::anyhow!(
                    "LaunchAgent is not installed. Run `irosh-server service install` first."
                ));
            }

            let bootstrap_status = std::process::Command::new("launchctl")
                .args(["bootstrap", &domain, &service_file.display().to_string()])
                .status()?;
            if !bootstrap_status.success() {
                std::process::Command::new("launchctl")
                    .args(["kickstart", "-k", &service_target])
                    .status()?;
            }

            println!("▶️ LaunchAgent started.");
        }
        ServiceAction::Stop => {
            std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status()?;
            println!("⏹️ LaunchAgent stopped.");
        }
        ServiceAction::Status => {
            if !service_file.exists() {
                println!("❌ LaunchAgent is not installed.");
                return Ok(());
            }

            let output = std::process::Command::new("launchctl")
                .args(["print", &service_target])
                .output()?;

            if output.status.success() {
                println!("✅ LaunchAgent '{}' is installed:", label);
                println!("{}", String::from_utf8_lossy(&output.stdout));
            } else {
                println!(
                    "ℹ️ LaunchAgent '{}' is installed but not currently loaded.",
                    label
                );
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    eprintln!("{}", stderr.trim());
                }
            }
        }
    }

    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn build_launchd_plist(
    exe: &std::path::Path,
    state: &Option<std::path::PathBuf>,
    user_home: &std::path::Path,
) -> String {
    let mut args = vec![plist_string(exe.display().to_string())];
    if let Some(state_dir) = state {
        args.push(plist_string("--state"));
        args.push(plist_string(state_dir.display().to_string()));
    }

    let stdout_path = plist_string(
        user_home
            .join("Library/Logs/irosh-server.log")
            .display()
            .to_string(),
    );
    let stderr_path = plist_string(
        user_home
            .join("Library/Logs/irosh-server.err.log")
            .display()
            .to_string(),
    );

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{MACOS_LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    {}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout_path}</string>
  <key>StandardErrorPath</key>
  <string>{stderr_path}</string>
</dict>
</plist>
"#,
        args.join("\n    ")
    )
}

#[cfg(any(target_os = "macos", test))]
fn plist_string(value: impl AsRef<str>) -> String {
    format!("<string>{}</string>", xml_escape(value.as_ref()))
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn current_uid() -> Result<String> {
    let output = std::process::Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "failed to resolve current user id with `id -u`"
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::{MACOS_LAUNCHD_FILE, MACOS_LAUNCHD_LABEL, build_launchd_plist};
    use std::path::Path;

    #[test]
    fn launchd_plist_includes_state_and_log_paths() {
        let plist = build_launchd_plist(
            Path::new("/Applications/irosh-server"),
            &Some("/tmp/irosh state".into()),
            Path::new("/Users/tester"),
        );

        assert!(plist.contains(MACOS_LAUNCHD_LABEL));
        assert!(plist.contains("<string>/Applications/irosh-server</string>"));
        assert!(plist.contains("<string>--state</string>"));
        assert!(plist.contains("<string>/tmp/irosh state</string>"));
        assert!(plist.contains("/Users/tester/Library/Logs/irosh-server.log"));
        assert!(plist.contains("/Users/tester/Library/Logs/irosh-server.err.log"));
        assert!(MACOS_LAUNCHD_FILE.ends_with(".plist"));
    }

    #[test]
    fn launchd_plist_escapes_xml_sensitive_values() {
        let plist = build_launchd_plist(
            Path::new("/tmp/irosh<&>\"'"),
            &Some("/tmp/state<&>\"'".into()),
            Path::new("/Users/tester<&>"),
        );

        assert!(plist.contains("/tmp/irosh&lt;&amp;&gt;&quot;&apos;"));
        assert!(plist.contains("/tmp/state&lt;&amp;&gt;&quot;&apos;"));
        assert!(plist.contains("/Users/tester&lt;&amp;&gt;/Library/Logs/irosh-server.log"));
    }
}
