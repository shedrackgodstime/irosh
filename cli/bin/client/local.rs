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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum LocalInputState {
    #[default]
    Normal,
    Escaped,
    Bracketed,
}

pub(super) struct LocalSessionState<'a> {
    pub(super) pending_line: &'a mut Vec<u8>,
    pub(super) local_command: &'a mut Option<Vec<u8>>,
    pub(super) transfer_context: &'a mut TransferContext,
    pub(super) input_state: &'a mut LocalInputState,
    pub(super) history: &'a mut crate::support::CommandHistory,
}

pub(super) async fn process_stdin_chunk(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    chunk: &[u8],
    state: &mut LocalSessionState<'_>,
) -> Result<InputOutcome> {
    for &byte in chunk {
        // If we are currently buffering a local command (started with ':').
        if let Some(buffer) = state.local_command.as_mut() {
            match *state.input_state {
                LocalInputState::Normal => match byte {
                    27 => {
                        *state.input_state = LocalInputState::Escaped;
                        continue;
                    }
                    b'\r' | b'\n' => {
                        stdout.write_all(b"\r\n").await?;
                        stdout.flush().await?;
                        let command = String::from_utf8_lossy(buffer).to_string();
                        state.history.add(&command);
                        state.history.reset();
                        *state.local_command = None;
                        state.pending_line.clear();
                        let outcome = match run_local_command(
                            session,
                            stdout,
                            &command,
                            state.transfer_context,
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
                    9 => {
                        // TAB - Simple Local Completion for :put
                        let current = String::from_utf8_lossy(buffer).to_string();
                        if current.starts_with(":put ") {
                            let parts: Vec<&str> = current.split_whitespace().collect();
                            let last_arg = if current.ends_with(' ') {
                                ""
                            } else {
                                parts.last().copied().unwrap_or("")
                            };

                            // Resolve what we have so far
                            let path = state.transfer_context.resolve_local_source(last_arg);
                            let (dir, prefix) = if path.is_dir() && last_arg.ends_with('/') {
                                (path, "")
                            } else {
                                (
                                    path.parent()
                                        .unwrap_or_else(|| std::path::Path::new("."))
                                        .to_path_buf(),
                                    path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                                )
                            };

                            if let Ok(entries) = std::fs::read_dir(dir) {
                                let matches: Vec<String> = entries
                                    .filter_map(|e| e.ok())
                                    .map(|e| e.file_name().to_string_lossy().to_string())
                                    .filter(|name| name.starts_with(prefix))
                                    .collect();

                                if matches.len() == 1 {
                                    let completion = &matches[0][prefix.len()..];
                                    let suffix = if std::fs::metadata(
                                        state.transfer_context.resolve_local_source(&format!(
                                            "{}{}",
                                            last_arg, completion
                                        )),
                                    )
                                    .map(|m| m.is_dir())
                                    .unwrap_or(false)
                                    {
                                        "/"
                                    } else {
                                        " "
                                    };

                                    let final_completion = format!("{}{}", completion, suffix);
                                    buffer.extend_from_slice(final_completion.as_bytes());
                                    stdout.write_all(final_completion.as_bytes()).await?;
                                    stdout.flush().await?;
                                }
                            }
                        }
                    }
                    _ => {
                        buffer.push(byte);
                        stdout.write_all(&[byte]).await?;
                        stdout.flush().await?;
                    }
                },
                LocalInputState::Escaped => {
                    if byte == b'[' || byte == b'O' {
                        *state.input_state = LocalInputState::Bracketed;
                    } else {
                        *state.input_state = LocalInputState::Normal;
                    }
                    continue;
                }
                LocalInputState::Bracketed => {
                    *state.input_state = LocalInputState::Normal;
                    let current = String::from_utf8_lossy(buffer).to_string();
                    let new_content = match byte {
                        b'A' => state.history.up(&current), // Up arrow
                        b'B' => state.history.down(),       // Down arrow
                        _ => None,
                    };

                    if let Some(new_cmd) = new_content {
                        // Clear current line on screen.
                        for _ in 0..buffer.len() {
                            stdout.write_all(b"\x08 \x08").await?;
                        }
                        // Update buffer.
                        buffer.clear();
                        buffer.extend_from_slice(new_cmd.as_bytes());
                        // Print new buffer.
                        stdout.write_all(buffer).await?;
                        stdout.flush().await?;
                    }
                    continue;
                }
            }
            continue;
        }

        // Check if this is the start of a local command.
        // We allow it if the current line is empty or only contains whitespace.
        let is_start_of_line = state.pending_line.is_empty()
            || state.pending_line.iter().all(|&b| b.is_ascii_whitespace());
        if is_start_of_line && byte == b':' {
            *state.local_command = Some(vec![byte]);
            stdout.write_all(b":").await?;
            stdout.flush().await?;
            continue;
        }

        // Regular character for the remote shell.
        session.send(&[byte]).await?;

        // Track user input to detect start-of-line for future ':' commands.
        match byte {
            b'\r' | b'\n' | 3 | 4 | 21 => {
                // Clear on Enter, Ctrl-C, Ctrl-D, or Ctrl-U
                state.pending_line.clear();
            }
            8 | 127 => {
                // Handle Backspace/Delete
                state.pending_line.pop();
            }
            _ => {
                state.pending_line.push(byte);
            }
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
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(InputOutcome::Continue);
    }
    let keyword = parts[0];

    match keyword {
        ":put" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if *arg == "-r" || *arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(*arg);
                }
            }

            if args.is_empty() {
                stdout
                    .write_all(b"Usage: :put [-r] <local> [remote]\r\n")
                    .await?;
                stdout.flush().await?;
                return Ok(InputOutcome::Continue);
            }
            let local = args[0];
            let remote = args.get(1).copied();
            handle_put_command(session, stdout, transfer_context, local, remote, recursive).await?;
        }
        ":get" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if *arg == "-r" || *arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(*arg);
                }
            }

            if args.is_empty() {
                stdout
                    .write_all(b"Usage: :get [-r] <remote> [local]\r\n")
                    .await?;
                stdout.flush().await?;
                return Ok(InputOutcome::Continue);
            }
            let remote = args[0];
            let local = args.get(1).copied();
            handle_get_command(session, stdout, transfer_context, remote, local, recursive).await?;
        }
        ":help" => {
            stdout
                .write_all(
                    b"Local client commands:\r\n  :pwd\r\n  :ls [path]\r\n  :cd <path>\r\n  :put [-r] <local> [remote]\r\n  :get [-r] <remote> [local]\r\n  :paths\r\n  :disconnect\r\n  :help\r\n",
                )
                .await?;
        }
        ":pwd" => {
            stdout
                .write_all(format!("{}\r\n", transfer_context.local_root.display()).as_bytes())
                .await?;
        }
        ":ls" => {
            let raw = parts.get(1);
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
            let Some(raw) = parts.get(1) else {
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
