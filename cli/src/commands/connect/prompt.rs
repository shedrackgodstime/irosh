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
        "lcd" | "cd" | "~lcd" | "~cd" => parts.get(1).map(|p| LocalCommand::Lcd(p.clone())),
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
                Some(LocalCommand::Unknown("put (missing path)".to_string()))
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
                Some(LocalCommand::Unknown("get (missing path)".to_string()))
            }
        }
        _ => Some(LocalCommand::Unknown(keyword.to_string())),
    }
}

/// Executes a parsed local command.
/// Returns `Ok(true)` if the session should continue, or `Ok(false)` if the session should disconnect.
pub async fn execute_local_command(
    session: &mut Session,
    input_engine: &mut InputEngine,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut irosh::sys::AsyncStdin,
    transfer_context: &mut TransferContext,
    cmd: LocalCommand,
) -> Result<bool> {
    macro_rules! print_prompt {
        () => {
            if input_engine.mode == InputMode::LocalEdit {
                let _ = stdout.write_all(b"\r\n\r\nirosh> ").await;
                let _ = stdout.flush().await;
            } else {
                // Executed from an escape sequence (e.g. ~put)
                // Send \r so the remote shell reprints its prompt.
                // We assume the local command already printed its own \r\n.
                let _ = session.send(b"\r").await;
            }
        };
    }

    match cmd {
        LocalCommand::Help => {
            stdout.write_all(b"Local prompt commands:\r\n  put [-r] <local> [remote]   Upload a file or directory to the remote peer.\r\n  get [-r] <remote> [local]   Download a file or directory from the remote peer.\r\n  lpwd                        Print the current local transfer directory.\r\n  lls [path]                  List files in a local directory.\r\n  lcd <path>                  Change the local transfer directory.\r\n  paths                       Show both local and remote transfer roots.\r\n  clear                       Clear the local screen.\r\n  exit                        Leave the irosh> prompt.\r\n  disconnect                  Close the session entirely.\r\n").await?;
            print_prompt!();
        }
        LocalCommand::Exit => {
            // Send \r so the remote shell reprints its prompt.
            let _ = session.send(b"\r").await;
            return Ok(true);
        }
        LocalCommand::Clear => {
            // Clear screen and move cursor to home
            let _ = stdout.write_all(b"\x1b[2J\x1b[H").await;
            print_prompt!();
        }
        LocalCommand::Disconnect => {
            stdout.write_all(b"[irosh] Disconnecting...\r\n").await?;
            stdout.flush().await?;
            return Ok(false);
        }
        LocalCommand::Lpwd => {
            let path = transfer_context.local_root.display();
            stdout
                .write_all(format!("Local working directory: {}\r\n", path).as_bytes())
                .await?;
            print_prompt!();
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
        }
        LocalCommand::Lls(path_opt) => {
            let target = match &path_opt {
                Some(p) => transfer_context.resolve_local_source(p),
                None => transfer_context.local_root.clone(),
            };
            if !target.is_dir() {
                stdout
                    .write_all(
                        format!("Error: '{}' is not a local directory", target.display())
                            .as_bytes(),
                    )
                    .await?;
                print_prompt!();
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
                let mut out = String::new();
                for entry in entries {
                    out.push_str(&entry);
                    out.push_str("\r\n");
                }
                stdout.write_all(out.trim_end().as_bytes()).await?;
                print_prompt!();
            }
        }
        LocalCommand::Paths => {
            let local = transfer_context.local_root.display();
            let remote = session
                .remote_cwd()
                .await
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            stdout
                .write_all(
                    format!(
                        "Local transfer root: {}\r\nRemote transfer root: {}",
                        local, remote
                    )
                    .as_bytes(),
                )
                .await?;
            print_prompt!();
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
        }
    }
    Ok(true)
}
