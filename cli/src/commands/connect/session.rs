use anyhow::Result;
use irosh::sys::AsyncStdin;
use irosh::{Session, SessionEvent};
use tokio::io::AsyncWriteExt;

use super::input::{EscapeAction, InputEngine};
use super::prompt::execute_local_command;
use super::transfer::TransferContext;

pub async fn drive_session(mut session: Session, mut input_engine: InputEngine) -> Result<()> {
    let mut stdin = AsyncStdin::new()?;
    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut transfer_context = TransferContext::new();
    let mut remote_buffer = Vec::new();

    loop {
        tokio::select! {
            // EVENTS from stdin: keystrokes OR resizes
            event = stdin.next_event() => {
                match event {
                    Some(irosh::sys::TerminalEvent::Data(data)) => {
                        let (to_remote, to_local, actions) =
                            input_engine.process_local(&data);

                        if !to_remote.is_empty() {
                            session.send(&to_remote).await?;
                        }

                        // Process actions.
                        for _action in &actions {
                            // Currently no actions require local terminal side-effects
                        }

                        // Local echo/erase feedback (now guaranteed to be on the correct buffer)
                        if !to_local.is_empty() {
                            stdout.write_all(&to_local).await?;
                            stdout.flush().await?;
                        }

                        for action in actions {
                            match action {
                                EscapeAction::Disconnect => {
                                    stdout
                                        .write_all(b"[irosh] Disconnecting...\r\n")
                                        .await?;
                                    stdout.flush().await?;
                                    return Ok(());
                                }
                                EscapeAction::Help => {
                                    show_help(&mut stdout).await?;
                                    let _ = stdout.write_all(b"\r\n").await;
                                    let _ = stdout.flush().await;

                                    if !remote_buffer.is_empty() {
                                        stdout.write_all(&remote_buffer).await?;
                                        stdout.flush().await?;
                                        remote_buffer.clear();
                                    }
                                }
                                EscapeAction::CommandPrompt => {
                                    // Already handled above to avoid race condition.
                                }
                                EscapeAction::RunLocal(cmd) => {
                                    if !execute_local_command(&mut session, &mut input_engine, &mut stdout, &mut stdin, &mut transfer_context, cmd).await? {
                                        return Ok(());
                                    }

                                    if input_engine.mode == super::input::InputMode::Remote && !remote_buffer.is_empty() {
                                        stdout.write_all(&remote_buffer).await?;
                                        stdout.flush().await?;
                                        remote_buffer.clear();
                                    }
                                }
                                EscapeAction::RequestCompletion => {
                                    let feedback = input_engine.complete_active_line(&mut session, &transfer_context).await;
                                    if !feedback.is_empty() {
                                        stdout.write_all(&feedback).await?;
                                        stdout.flush().await?;
                                    }
                                }
                            }
                        }
                    }
                    Some(irosh::sys::TerminalEvent::Resize(size)) => {
                        let _ = session.resize(size).await;
                        if let Some(feedback) = input_engine.handle_resize() {
                            let _ = stdout.write_all(&feedback).await;
                            let _ = stdout.flush().await;
                        }
                    }
                    None => {
                        session.eof().await?;
                        break;
                    }
                }
            }

            // DATA and RESIZE from the remote session.
            event = session.next_event() => {
                match event? {
                    Some(SessionEvent::Data(data)) => {
                        input_engine.observe_remote(&data);
                        if input_engine.mode == super::input::InputMode::LocalEdit {
                            // Buffer remote data while local prompt is active to prevent screen corruption.
                            remote_buffer.extend_from_slice(&data);
                        } else {
                            stdout.write_all(&data).await?;
                            stdout.flush().await?;
                        }
                    }
                    Some(SessionEvent::ExtendedData(data, _)) => {
                        stderr.write_all(&data).await?;
                        stderr.flush().await?;
                    }
                    Some(SessionEvent::Closed) => {
                        stdout.write_all(b"\r\nSession closed.\r\n").await?;
                        stdout.flush().await?;
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
        }
    }

    let _ = session.disconnect().await;
    Ok(())
}

async fn show_help(stdout: &mut tokio::io::Stdout) -> Result<()> {
    stdout
        .write_all(b"[irosh] Supported Escape Sequences:\r\n")
        .await?;
    stdout
        .write_all(b"  ~.  - Terminate connection\r\n")
        .await?;
    stdout
        .write_all(b"  ~C  - Open local command prompt\r\n")
        .await?;
    stdout
        .write_all(b"  ~?  - Display this help message\r\n")
        .await?;
    stdout
        .write_all(b"  ~~  - Send a literal tilde character\r\n")
        .await?;
    stdout
        .write_all(b"(Escape sequences are only recognized at the start of a line)\r\n")
        .await?;
    stdout.flush().await?;
    Ok(())
}
