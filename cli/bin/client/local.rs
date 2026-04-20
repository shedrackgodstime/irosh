use anyhow::Result;
use irosh::Session;
use tokio::io::AsyncWriteExt;

use crate::support::{
    best_error_message, format_local_listing, looks_like_directory, normalize_path,
    resolve_local_input_path, strip_ansi,
};
use crate::transfer::{auto_rename_download_target, handle_get_command, handle_put_command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputOutcome {
    Continue,
    Disconnect,
}

#[derive(Debug, Clone)]
pub(super) struct TransferContext {
    pub(super) local_root: std::path::PathBuf,
}

impl TransferContext {
    pub(super) fn resolve_local_source(&self, raw: &str) -> std::path::PathBuf {
        resolve_local_input_path(&self.local_root, raw)
    }

    pub(super) fn resolve_local_target(
        &self,
        raw: Option<&str>,
        fallback_name: &str,
    ) -> std::path::PathBuf {
        match raw {
            None => self.local_root.join(fallback_name),
            Some(raw) if raw == "." || raw == "./" => self.local_root.join(fallback_name),
            Some(raw) => {
                let path = resolve_local_input_path(&self.local_root, raw);
                if looks_like_directory(raw, &path) {
                    auto_rename_download_target(path.join(fallback_name))
                } else {
                    path
                }
            }
        }
    }
}

pub(super) async fn process_stdin_chunk(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    chunk: &[u8],
    pending_line: &mut Vec<u8>,
    local_command: &mut Option<Vec<u8>>,
    transfer_context: &mut TransferContext,
) -> Result<InputOutcome> {
    for &byte in chunk {
        if let Some(buffer) = local_command.as_mut() {
            match byte {
                b'\r' | b'\n' => {
                    stdout.write_all(b"\r\n").await?;
                    stdout.flush().await?;
                    let command = String::from_utf8_lossy(buffer).to_string();
                    *local_command = None;
                    pending_line.clear();
                    let outcome = match run_local_command(
                        session,
                        stdout,
                        &command,
                        transfer_context,
                    )
                    .await
                    {
                        Ok(outcome) => outcome,
                        Err(err) => {
                            let msg =
                                format!("\r\nError: {}\r\n", best_error_message(err.as_ref()));
                            stdout.write_all(msg.as_bytes()).await?;
                            stdout.flush().await?;
                            InputOutcome::Continue
                        }
                    };
                    if matches!(outcome, InputOutcome::Disconnect) {
                        return Ok(outcome);
                    }
                }
                8 | 127 => {
                    if buffer.len() > 1 {
                        buffer.pop();
                        stdout.write_all(b"\x08 \x08").await?;
                        stdout.flush().await?;
                    }
                }
                _ => {
                    buffer.push(byte);
                    stdout.write_all(&[byte]).await?;
                    stdout.flush().await?;
                }
            }
            continue;
        }

        if pending_line.is_empty() && byte == b':' {
            *local_command = Some(vec![byte]);
            stdout.write_all(b":").await?;
            stdout.flush().await?;
            continue;
        }

        session.send(&[byte]).await?;
        match byte {
            b'\r' | b'\n' => pending_line.clear(),
            8 | 127 => {
                pending_line.pop();
            }
            _ => pending_line.push(byte),
        }
    }

    Ok(InputOutcome::Continue)
}

async fn run_local_command(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    command: &str,
    transfer_context: &mut TransferContext,
) -> Result<InputOutcome> {
    let cleaned = strip_ansi(command);
    let trimmed = cleaned.trim();
    let mut parts = trimmed.split_whitespace();
    let keyword = parts.next().unwrap_or_default();

    match keyword {
        ":put" => {
            let Some(local) = parts.next() else {
                stdout
                    .write_all(b"Usage: :put <local> [remote]\r\n")
                    .await?;
                stdout.flush().await?;
                return Ok(InputOutcome::Continue);
            };
            let remote = parts.next();
            handle_put_command(session, stdout, transfer_context, local, remote).await?;
        }
        ":get" => {
            let Some(remote) = parts.next() else {
                stdout
                    .write_all(b"Usage: :get <remote> [local]\r\n")
                    .await?;
                stdout.flush().await?;
                return Ok(InputOutcome::Continue);
            };
            let local = parts.next();
            handle_get_command(session, stdout, transfer_context, remote, local).await?;
        }
        ":help" => {
            stdout
                .write_all(
                    b"Local client commands:\r\n  :pwd\r\n  :ls [path]\r\n  :cd <path>\r\n  :put <local> [remote]\r\n  :get <remote> [local]\r\n  :paths\r\n  :disconnect\r\n  :help\r\n",
                )
                .await?;
        }
        ":pwd" => {
            stdout
                .write_all(format!("{}\r\n", transfer_context.local_root.display()).as_bytes())
                .await?;
        }
        ":ls" => {
            let raw = parts.next();
            let path = match raw {
                Some(value) => resolve_local_input_path(&transfer_context.local_root, value),
                None => transfer_context.local_root.clone(),
            };

            match format_local_listing(&path) {
                Ok(output) => stdout.write_all(output.as_bytes()).await?,
                Err(err) => {
                    stdout
                        .write_all(
                            format!(
                                "Local listing failed.\r\nPath: {}\r\nError: {:#}\r\n",
                                path.display(),
                                err
                            )
                            .as_bytes(),
                        )
                        .await?;
                }
            }
        }
        ":cd" => {
            let Some(raw) = parts.next() else {
                stdout.write_all(b"Usage: :cd <path>\r\n").await?;
                stdout.flush().await?;
                return Ok(InputOutcome::Continue);
            };

            let path = resolve_local_input_path(&transfer_context.local_root, raw);
            match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_dir() => {
                    transfer_context.local_root = normalize_path(path.clone());
                    stdout
                        .write_all(
                            format!("Local cwd: {}\r\n", transfer_context.local_root.display())
                                .as_bytes(),
                        )
                        .await?;
                }
                Ok(_) => {
                    stdout
                        .write_all(
                            format!(
                                "Local cd failed.\r\nPath: {}\r\nError: not a directory\r\n",
                                path.display()
                            )
                            .as_bytes(),
                        )
                        .await?;
                }
                Err(err) => {
                    stdout
                        .write_all(
                            format!(
                                "Local cd failed.\r\nPath: {}\r\nError: {}\r\n",
                                path.display(),
                                err
                            )
                            .as_bytes(),
                        )
                        .await?;
                }
            }
        }
        ":paths" => {
            let remote_cwd = match session.remote_cwd().await {
                Ok(path) => path.display().to_string(),
                Err(err) => format!("unavailable ({})", best_error_message(&err)),
            };
            stdout
                .write_all(
                    format!(
                        "Local transfer cwd: {}\r\nRemote transfer cwd: {}\r\n",
                        transfer_context.local_root.display(),
                        remote_cwd
                    )
                    .as_bytes(),
                )
                .await?;
        }
        ":disconnect" => {
            stdout
                .write_all(b"Disconnecting local session...\r\n")
                .await?;
            stdout.flush().await?;
            let _ = session.disconnect().await;
            return Ok(InputOutcome::Disconnect);
        }
        ":" | "" => {}
        _ => {
            stdout
                .write_all(
                    b"Unknown local command. Type ':help' for available client commands.\r\n",
                )
                .await?;
        }
    }

    stdout.flush().await?;
    Ok(InputOutcome::Continue)
}
