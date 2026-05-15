use anyhow::Result;
use irosh::Session;
use tokio::io::AsyncWriteExt;

use super::input::{InputEngine, InputMode};
use super::transfer::{TransferContext, handle_get_command, handle_put_command};

/// A command executed from the local `irosh>` prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalCommand {
    Help,
    Lpwd,
    Lls(Option<String>),
    Lcd(String),
    Paths,
    Exit,
    Disconnect,
    Clear,
    Put {
        local: String,
        remote: Option<String>,
        recursive: bool,
    },
    Get {
        remote: String,
        local: Option<String>,
        recursive: bool,
    },
    UsageError(String),
    Unknown(String),
}

/// Parses a line typed at the `irosh>` prompt into a `LocalCommand`.
pub fn parse_local_command(buf: &[u8]) -> Option<LocalCommand> {
    let line = String::from_utf8_lossy(buf);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parts = shell_words::split(trimmed).unwrap_or_default();
    let keyword = parts.first()?.as_str();

    match keyword {
        "help" | "?" | "~?" | "~help" => Some(LocalCommand::Help),
        "lpwd" | "pwd" | "~lpwd" | "~pwd" => Some(LocalCommand::Lpwd),
        "lls" | "ls" | "~lls" | "~ls" => Some(LocalCommand::Lls(parts.get(1).cloned())),
        "lcd" | "cd" | "~lcd" | "~cd" => {
            if let Some(path) = parts.get(1) {
                Some(LocalCommand::Lcd(path.clone()))
            } else {
                Some(LocalCommand::UsageError(
                    "lcd: missing directory path. Usage: lcd <path>".to_string(),
                ))
            }
        }
        "paths" => Some(LocalCommand::Paths),
        "exit" | "~exit" => Some(LocalCommand::Exit),
        "disconnect" | "~disconnect" => Some(LocalCommand::Disconnect),
        "clear" | "cls" | "~clear" | "~cls" => Some(LocalCommand::Clear),
        "put" | "~put" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if arg == "-r" || arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(arg.clone());
                }
            }
            if let Some(local) = args.first() {
                Some(LocalCommand::Put {
                    local: local.clone(),
                    remote: args.get(1).cloned(),
                    recursive,
                })
            } else {
                Some(LocalCommand::UsageError(
                    "put: missing local path. Usage: put [-r] <local> [remote]".to_string(),
                ))
            }
        }
        "get" | "~get" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if arg == "-r" || arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(arg.clone());
                }
            }
            if let Some(remote) = args.first() {
                Some(LocalCommand::Get {
                    remote: remote.clone(),
                    local: args.get(1).cloned(),
                    recursive,
                })
            } else {
                Some(LocalCommand::UsageError(
                    "get: missing remote path. Usage: get [-r] <remote> [local]".to_string(),
                ))
            }
        }
        _ => Some(LocalCommand::Unknown(keyword.to_string())),
    }
}

/// Executes a parsed local command.
/// Returns `Ok((continue_session, lines_printed))`.
pub async fn execute_local_command(
    session: &mut Session,
    input_engine: &mut InputEngine,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut irosh::sys::AsyncStdin,
    transfer_context: &mut TransferContext,
    cmd: LocalCommand,
) -> Result<(bool, u16)> {
    macro_rules! print_prompt {
        () => {
            if input_engine.mode == InputMode::LocalEdit {
                // We assume the previous command (or the shell) left us at the start of a line.
                let _ = stdout.write_all(b"\r\nirosh> ").await;
                let _ = stdout.flush().await;
            } else {
                // Return to remote shell: do NOT force remote reprint tightly.
                // See UX_GUIDELINES.md "The OpenSSH Pattern".
                let _ = stdout.flush().await;
            }
        };
    }

    match cmd {
        LocalCommand::Help => {
            stdout.write_all(b"Local prompt commands:\r\n  put [-r] <local> [remote]   Upload a file or directory to the remote peer.\r\n  get [-r] <remote> [local]   Download a file or directory from the remote peer.\r\n  lpwd                        Print the current local transfer directory.\r\n  lls [path]                  List files in a local directory.\r\n  lcd <path>                  Change the local transfer directory.\r\n  paths                       Show both local and remote transfer roots.\r\n  clear                       Clear the local screen.\r\n  exit                        Leave the irosh> prompt.\r\n  disconnect                  Close the session entirely.\r\n").await?;
            print_prompt!();
            Ok((true, 11))
        }
        LocalCommand::Exit => {
            // We are already on the line below the command (Enter moved us there).
            // Do NOT send \r to the remote. See UX_GUIDELINES.md "The OpenSSH Pattern".
            Ok((true, 0))
        }
        LocalCommand::Clear => {
            // Clear screen and move cursor to home
            let _ = stdout.write_all(b"\x1b[2J\x1b[H").await;
            print_prompt!();
            Ok((true, 0)) // effectively 0 since it clears
        }
        LocalCommand::Disconnect => {
            stdout.write_all(b"[irosh] Disconnecting...\r\n").await?;
            stdout.flush().await?;
            Ok((false, 1))
        }
        LocalCommand::Lpwd => {
            let path = transfer_context.local_root.display();
            stdout
                .write_all(format!("Local working directory: {}\r\n", path).as_bytes())
                .await?;
            print_prompt!();
            Ok((true, 1))
        }
        LocalCommand::Lcd(path) => {
            let new_path = transfer_context.resolve_local_source(&path);
            if new_path.is_dir() {
                transfer_context.local_root = new_path.clone();
                stdout
                    .write_all(
                        format!("Changed local directory to: {}\r\n", new_path.display())
                            .as_bytes(),
                    )
                    .await?;
            } else {
                stdout
                    .write_all(
                        format!("Error: '{}' is not a valid local directory\r\n", path).as_bytes(),
                    )
                    .await?;
            }
            print_prompt!();
            Ok((true, 1))
        }
        LocalCommand::Lls(path_opt) => {
            let target = match &path_opt {
                Some(p) => transfer_context.resolve_local_source(p),
                None => transfer_context.local_root.clone(),
            };
            if !target.is_dir() {
                stdout
                    .write_all(
                        format!("Error: '{}' is not a local directory\r\n", target.display())
                            .as_bytes(),
                    )
                    .await?;
                print_prompt!();
                Ok((true, 1))
            } else {
                let mut entries = Vec::new();
                if let Ok(mut dir) = tokio::fs::read_dir(&target).await {
                    while let Ok(Some(entry)) = dir.next_entry().await {
                        if let Ok(name) = entry.file_name().into_string() {
                            let prefix =
                                if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                                    "[DIR] "
                                } else {
                                    "      "
                                };
                            entries.push(format!("{}{}", prefix, name));
                        }
                    }
                }
                entries.sort();
                let lines_printed = entries.len() as u16;
                let mut out = String::new();
                for entry in entries {
                    out.push_str(&entry);
                    out.push_str("\r\n");
                }
                stdout.write_all(out.as_bytes()).await?;
                print_prompt!();
                Ok((true, lines_printed))
            }
        }
        LocalCommand::Paths => {
            let local = transfer_context.local_root.display();
            let remote =
                tokio::time::timeout(std::time::Duration::from_secs(30), session.remote_cwd())
                    .await
                    .map(|res| match res {
                        Ok(p) => p.display().to_string(),
                        Err(_) => "unknown (error)".to_string(),
                    })
                    .unwrap_or_else(|_| "unknown (timeout)".to_string());

            stdout
                .write_all(
                    format!(
                        "Local transfer root: {}\r\nRemote transfer root: {}\r\n",
                        local, remote
                    )
                    .as_bytes(),
                )
                .await?;
            print_prompt!();
            Ok((true, 2))
        }
        LocalCommand::Put {
            local,
            remote,
            recursive,
        } => {
            handle_put_command(
                session,
                stdout,
                stdin,
                transfer_context,
                &local,
                remote.as_deref(),
                recursive,
            )
            .await?;
            print_prompt!();
            Ok((true, 2)) // progress bars usually take 1-2 lines
        }
        LocalCommand::Get {
            remote,
            local,
            recursive,
        } => {
            handle_get_command(
                session,
                stdout,
                stdin,
                transfer_context,
                &remote,
                local.as_deref(),
                recursive,
            )
            .await?;
            print_prompt!();
            Ok((true, 2))
        }
        LocalCommand::UsageError(msg) => {
            stdout
                .write_all(format!("Error: {}\r\n", msg).as_bytes())
                .await?;
            print_prompt!();
            Ok((true, 1))
        }
        LocalCommand::Unknown(cmd) => {
            stdout
                .write_all(
                    format!(
                        "Unknown command: '{}'. Type 'help' for available commands.\r\n",
                        cmd
                    )
                    .as_bytes(),
                )
                .await?;
            print_prompt!();
            Ok((true, 1))
        }
    }
}
