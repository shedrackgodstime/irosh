use anyhow::Result;
use irosh::russh::keys::ssh_key::HashAlg;
use irosh::{ClientOptions, Session, SessionState};

use crate::commands::connect::support::choose_auto_alias;

pub(super) async fn handle_connect_error(
    err: irosh::error::IroshError,
    options: &ClientOptions,
) -> Result<Session> {
    match err {
        irosh::error::IroshError::ServerKeyMismatch { expected, actual } => {
            eprintln!("\n🚨 SECURITY ALERT: SERVER IDENTIFICATION HAS CHANGED!");
            eprintln!("--------------------------------------------------");
            eprintln!("The server is identifying itself as: {}", actual);
            eprintln!("But your local records expected:     {}", expected);
            eprintln!(
                "\nIf you know the server has been re-installed, you can reset trust by deleting:"
            );
            eprintln!("  {}/known_server", options.state().root().display());
            eprintln!("--------------------------------------------------\n");
            Err(anyhow::anyhow!(
                "Connection blocked for security. Identification mismatch."
            ))
        }
        irosh::error::IroshError::AuthenticationFailed => {
            let identity = irosh::storage::load_or_generate_identity(options.state()).await?;
            let fingerprint = identity.ssh_key.public_key().fingerprint(HashAlg::Sha256);

            eprintln!("\n🚫 AUTHENTICATION REJECTED BY SERVER");
            eprintln!("--------------------------------------------------");
            eprintln!("The server refused this client key.");
            eprintln!("This usually means one of these is true:");
            eprintln!("  1. the server already trusts a different client key");
            eprintln!("  2. the server requires explicit authorization");
            eprintln!("Your client fingerprint: {}", fingerprint);
            eprintln!("\nShow your public key for whitelisting with:");
            eprintln!("  irosh identity");
            eprintln!("--------------------------------------------------\n");

            Err(anyhow::anyhow!(
                "Connection blocked by the server authentication policy."
            ))
        }
        irosh::error::IroshError::Client(irosh::error::ClientError::SshNegotiationFailed {
            ..
        }) => {
            let identity = irosh::storage::load_or_generate_identity(options.state()).await?;
            let fingerprint = identity.ssh_key.public_key().fingerprint(HashAlg::Sha256);

            eprintln!("\n🚫 SSH HANDSHAKE FAILED");
            eprintln!("--------------------------------------------------");
            eprintln!("The SSH peer disconnected during handshake.");
            eprintln!("\nMost likely causes:");
            eprintln!("  1. the server rejected this client key");
            eprintln!("  2. the server crashed or aborted during SSH setup");
            eprintln!("Your client fingerprint: {}", fingerprint);
            eprintln!("\nShow your public key for whitelisting with:");
            eprintln!("  irosh identity");
            eprintln!("If you control the server, rerun it with --verbose and inspect its logs.");
            eprintln!("--------------------------------------------------\n");

            Err(anyhow::anyhow!("SSH handshake failed."))
        }
        other => Err(other.into()),
    }
}

pub(super) fn maybe_autosave_alias(
    session: &Session,
    options: &ClientOptions,
    target_str: &str,
) -> Result<()> {
    if let Some(meta) = session.remote_metadata() {
        let is_saved_alias = irosh::storage::get_peer(options.state(), target_str)?.is_some();
        if !is_saved_alias {
            let default_alias =
                choose_auto_alias(meta.default_alias().as_str(), target_str).to_string();
            match irosh::storage::get_peer(options.state(), &default_alias)? {
                Some(existing) if existing.ticket.to_string() == *target_str => {
                    println!(
                        "ℹ️ Alias '{}' already points to this peer. Leaving it unchanged.",
                        default_alias
                    );
                }
                Some(_) => {
                    println!(
                        "ℹ️ Alias '{}' is already in use locally. Skipping auto-save.",
                        default_alias
                    );
                }
                None => {
                    irosh::storage::save_peer(
                        options.state(),
                        &irosh::storage::PeerProfile {
                            name: default_alias.clone(),
                            ticket: target_str.parse()?,
                        },
                    )?;
                    println!(
                        "✨ Auto-saved peer alias: You can now connect using 'irosh {}'",
                        default_alias
                    );
                }
            }
        }
    }

    if matches!(session.state(), SessionState::Authenticated) {
        println!("🔒 Secure session established.");
    }

    Ok(())
}
