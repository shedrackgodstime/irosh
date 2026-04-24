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
    pub(super) workspace_root: std::path::PathBuf,
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

pub(super) async fn process_stdin_chunk<S>(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut S,
    chunk: &[u8],
    state: &mut LocalSessionState<'_>,
) -> Result<InputOutcome>
where
    S: tokio::io::AsyncRead + Unpin,
{
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
                            stdin,
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
                        // Nudge the remote shell to re-print the prompt.
                        let _ = session.send(b"\r").await;
                    }
                    8 | 127 => {
                        // Backspace/Delete
                        if !buffer.is_empty() {
                            buffer.pop();
                            if buffer.is_empty() {
                                // Deleted the leading ':', exit local mode
                                *state.local_command = None;
                            }
                            stdout.write_all(b"\x08 \x08").await?;
                            stdout.flush().await?;
                        }
                    }
                    9 => {
                        // TAB - Completion
                        let current_line = String::from_utf8_lossy(buffer).to_string();
                        let parts = shell_words::split(&current_line).unwrap_or_default();

                        if current_line.starts_with(":put ") {
                            let last_arg = if current_line.ends_with(' ') {
                                "".to_string()
                            } else {
                                parts.last().cloned().unwrap_or_default()
                            };

                            let path = state.transfer_context.resolve_local_source(&last_arg);
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
                                    let full_local_path =
                                        state.transfer_context.resolve_local_source(&format!(
                                            "{}{}",
                                            last_arg, completion
                                        ));
                                    let suffix = if full_local_path.is_dir() { "/" } else { " " };
                                    let final_completion = format!("{}{}", completion, suffix);

                                    buffer.extend_from_slice(final_completion.as_bytes());
                                    stdout.write_all(final_completion.as_bytes()).await?;
                                    stdout.flush().await?;
                                } else if !matches.is_empty() {
                                    let msg = format!("\r\n{}\r\n:", matches.join("  "));
                                    stdout.write_all(msg.as_bytes()).await?;
                                    stdout.write_all(&buffer[1..]).await?;
                                    stdout.flush().await?;
                                }
                            }
                        } else if current_line.starts_with(":get ") {
                            let last_arg = if current_line.ends_with(' ') {
                                "".to_string()
                            } else {
                                parts.last().cloned().unwrap_or_default()
                            };

                            if let Ok(matches) = session.remote_completion(&last_arg).await {
                                if matches.len() == 1 {
                                    let full_match = &matches[0];
                                    let completion = if full_match.starts_with(&last_arg) {
                                        &full_match[last_arg.len()..]
                                    } else {
                                        full_match
                                    };
                                    buffer.extend_from_slice(completion.as_bytes());
                                    stdout.write_all(completion.as_bytes()).await?;
                                    stdout.flush().await?;
                                } else if !matches.is_empty() {
                                    // Multiple matches: show them?
                                    let msg = format!("\r\n{}\r\n:", matches.join("  "));
                                    stdout.write_all(msg.as_bytes()).await?;
                                    stdout.write_all(&buffer[1..]).await?; // Re-print current command
                                    stdout.flush().await?;
                                }
                            }
                        } else if parts.len() == 1 && !current_line.ends_with(' ') {
                            // Complete the command itself
                            let cmd = &parts[0];
                            let commands = vec![
                                ":put",
                                ":get",
                                ":pwd",
                                ":ls",
                                ":cd",
                                ":paths",
                                ":help",
                                ":disconnect",
                            ];
                            let matches: Vec<&&str> =
                                commands.iter().filter(|c| c.starts_with(cmd)).collect();
                            if matches.len() == 1 {
                                let completion = &matches[0][cmd.len()..];
                                let suffix = " ";
                                buffer.extend_from_slice(completion.as_bytes());
                                buffer.extend_from_slice(suffix.as_bytes());
                                stdout.write_all(completion.as_bytes()).await?;
                                stdout.write_all(suffix.as_bytes()).await?;
                                stdout.flush().await?;
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
                        // Use robust ANSI: Clear entire line and return to start
                        stdout.write_all(b"\x1b[2K\r").await?;
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

async fn run_local_command<S>(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut S,
    command: &str,
    transfer_context: &mut TransferContext,
) -> Result<InputOutcome>
where
    S: tokio::io::AsyncRead + Unpin,
{
    let cleaned = strip_ansi(command);
    let trimmed = cleaned.trim();

    let parts = match shell_words::split(trimmed) {
        Ok(p) => p,
        Err(e) => {
            stdout
                .write_all(format!("Invalid command: {e}\r\n").as_bytes())
                .await?;
            return Ok(InputOutcome::Continue);
        }
    };

    if parts.is_empty() {
        return Ok(InputOutcome::Continue);
    }
    let keyword = &parts[0];

    match keyword.as_str() {
        ":put" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if arg == "-r" || arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(arg);
                }
            }

            if args.is_empty() {
                stdout
                    .write_all(b"Usage: :put [-r] <local> [remote]\r\n")
                    .await?;
                stdout.flush().await?;
                return Ok(InputOutcome::Continue);
            }
            let local = &args[0];
            let remote = args.get(1).map(|s| s.as_str());
            handle_put_command(
                session,
                stdout,
                stdin,
                transfer_context,
                local,
                remote,
                recursive,
            )
            .await?;
        }
        ":get" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if arg == "-r" || arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(arg);
                }
            }

            if args.is_empty() {
                stdout
                    .write_all(b"Usage: :get [-r] <remote> [local]\r\n")
                    .await?;
                stdout.flush().await?;
                return Ok(InputOutcome::Continue);
            }
            let remote = &args[0];
            let local = args.get(1).map(|s| s.as_str());
            handle_get_command(
                session,
                stdout,
                stdin,
                transfer_context,
                remote,
                local,
                recursive,
            )
            .await?;
        }
        ":help" | ":?" => {
            let help_text = r#"
📡 Irosh Local Commands:
  :pwd                    Print the current local working directory.
  :ls [path]              List files in the local directory.
  :cd <path>              Change the local working directory.
  :paths                  Show both local and remote transfer roots.
  :disconnect             Close the P2P session and exit.
  :help, :?               Show this help menu.

📂 File Transfers:
  :put [-r] <src> [dst]   Upload a file or directory to the remote peer.
  :get [-r] <src> [dst]   Download a file or directory from the remote peer.

💡 Options & Examples:
  -r, --recursive         Transfer an entire directory recursively.
  
  Example: :put -r ./assets /tmp/assets
  Example: :get notes.txt ./backup.txt

(Tip: Use Tab completion for local paths in :put)
"#;
            // Convert to CRLF for raw terminal compatibility
            let formatted = help_text.replace("\n", "\r\n");
            stdout.write_all(formatted.as_bytes()).await?;
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
            let normalized_path = normalize_path(path.clone());

            if !normalized_path.starts_with(&transfer_context.workspace_root) {
                stdout
                    .write_all(
                        format!(
                            "Local cd failed.\r\nError: Path traversal attempt blocked.\r\nPath '{}' is outside workspace root '{}'\r\n",
                            normalized_path.display(),
                            transfer_context.workspace_root.display()
                        )
                        .as_bytes(),
                    )
                    .await?;
                stdout.flush().await?;
                return Ok(InputOutcome::Continue);
            }

            match std::fs::metadata(&normalized_path) {
                Ok(metadata) if metadata.is_dir() => {
                    transfer_context.local_root = normalized_path;
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
                    b"Unknown local command. Type ':help' or ':?' for available client commands.\r\n",
                )
                .await?;
        }
    }

    stdout.flush().await?;
    Ok(InputOutcome::Continue)
}
