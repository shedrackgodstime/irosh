use anyhow::Result;
use irosh::Session;
use tokio::io::AsyncWriteExt;

use super::display::print_local_block;
use crate::commands::connect::support::{
    best_error_message, format_local_listing, normalize_path, resolve_local_input_path,
};
use crate::commands::connect::transfer::{TransferContext, handle_get_command, handle_put_command};

pub(super) const LOCAL_PROMPT: &str = "irosh> ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptOutcome {
    Continue,
    Exit,
    Disconnect,
}

pub(super) async fn run_prompt_command<S>(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut S,
    transfer_context: &mut TransferContext,
    command: &[u8],
) -> Result<PromptOutcome>
where
    S: tokio::io::AsyncRead + Unpin,
{
    let line = String::from_utf8_lossy(command).to_string();
    let trimmed = line.trim();

    let parts = match shell_words::split(trimmed) {
        Ok(parts) => parts,
        Err(err) => {
            print_local_block(stdout, &format!("Invalid command: {err}")).await?;
            return Ok(PromptOutcome::Continue);
        }
    };

    let Some(keyword) = parts.first().map(String::as_str) else {
        return Ok(PromptOutcome::Continue);
    };

    match keyword {
        "put" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if arg == "-r" || arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(arg.as_str());
                }
            }

            let Some(local) = args.first().copied() else {
                print_local_block(stdout, "Usage: put [-r] <local> [remote]").await?;
                return Ok(PromptOutcome::Continue);
            };

            if let Err(err) = handle_put_command(
                session,
                stdout,
                stdin,
                transfer_context,
                local,
                args.get(1).copied(),
                recursive,
            )
            .await
            {
                print_local_block(
                    stdout,
                    &format!("Error: {}", best_error_message(err.as_ref())),
                )
                .await?;
            }
        }
        "get" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if arg == "-r" || arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(arg.as_str());
                }
            }

            let Some(remote) = args.first().copied() else {
                print_local_block(stdout, "Usage: get [-r] <remote> [local]").await?;
                return Ok(PromptOutcome::Continue);
            };

            if let Err(err) = handle_get_command(
                session,
                stdout,
                stdin,
                transfer_context,
                remote,
                args.get(1).copied(),
                recursive,
            )
            .await
            {
                print_local_block(
                    stdout,
                    &format!("Error: {}", best_error_message(err.as_ref())),
                )
                .await?;
            }
        }
        "help" | "?" => {
            let help = r#"
Local prompt commands:
  put [-r] <local> [remote]   Upload a file or directory to the remote peer.
  get [-r] <remote> [local]   Download a file or directory from the remote peer.
  pwd                         Print the current local transfer directory.
  ls [path]                   List files in a local directory.
  cd <path>                   Change the local transfer directory.
  paths                       Show both local and remote transfer roots.
  exit                        Leave the irosh> prompt.
  disconnect                  Close the session entirely.
"#;
            print_local_block(stdout, help.trim()).await?;
        }
        "pwd" => {
            print_local_block(stdout, &transfer_context.local_root.display().to_string()).await?;
        }
        "ls" => {
            let path = match parts.get(1) {
                Some(raw) => resolve_local_input_path(&transfer_context.local_root, raw),
                None => transfer_context.local_root.clone(),
            };

            match format_local_listing(&path) {
                Ok(output) => {
                    print_local_block(stdout, output.trim_end_matches(['\r', '\n'])).await?;
                }
                Err(err) => {
                    print_local_block(
                        stdout,
                        &format!(
                            "Local listing failed.\nPath: {}\nError: {:#}",
                            path.display(),
                            err
                        ),
                    )
                    .await?;
                }
            }
        }
        "cd" => {
            let Some(raw) = parts.get(1) else {
                print_local_block(stdout, "Usage: cd <path>").await?;
                return Ok(PromptOutcome::Continue);
            };

            let path = resolve_local_input_path(&transfer_context.local_root, raw);
            let normalized = normalize_path(path.clone());

            match std::fs::metadata(&normalized) {
                Ok(metadata) if metadata.is_dir() => {
                    transfer_context.local_root = normalized;
                    print_local_block(
                        stdout,
                        &format!("Local cwd: {}", transfer_context.local_root.display()),
                    )
                    .await?;
                }
                Ok(_) => {
                    print_local_block(
                        stdout,
                        &format!(
                            "Local cd failed.\nPath: {}\nError: not a directory",
                            path.display()
                        ),
                    )
                    .await?;
                }
                Err(err) => {
                    print_local_block(
                        stdout,
                        &format!("Local cd failed.\nPath: {}\nError: {}", path.display(), err),
                    )
                    .await?;
                }
            }
        }
        "paths" => {
            let remote_cwd = match session.remote_cwd().await {
                Ok(path) => path.display().to_string(),
                Err(err) => format!("unavailable ({})", best_error_message(&err)),
            };
            print_local_block(
                stdout,
                &format!(
                    "Local transfer cwd: {}\nRemote transfer cwd: {}",
                    transfer_context.local_root.display(),
                    remote_cwd
                ),
            )
            .await?;
        }
        "exit" => return Ok(PromptOutcome::Exit),
        "disconnect" => {
            print_local_block(stdout, "Disconnecting local session...").await?;
            let _ = session.disconnect().await;
            return Ok(PromptOutcome::Disconnect);
        }
        _ => {
            print_local_block(
                stdout,
                "Unknown local command. Type 'help' or '?' for available prompt commands.",
            )
            .await?;
        }
    }

    stdout.flush().await?;
    Ok(PromptOutcome::Continue)
}
