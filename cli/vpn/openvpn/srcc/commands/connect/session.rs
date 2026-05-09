use anyhow::Result;
use irosh::sys::{AsyncStdin, current_terminal_size};
use irosh::{Session, SessionEvent};
use std::io::IsTerminal;
use tokio::io::AsyncWriteExt;

use super::input::{EscapeAction, InputEngine};
use super::prompt::execute_local_command;
use super::transfer::TransferContext;

pub async fn drive_session(mut session: Session, mut input_engine: InputEngine) -> Result<()> {
    let mut stdin = AsyncStdin::new()?;
    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut transfer_context = TransferContext::new();

    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    // On Unix, SIGWINCH tells us when the terminal is resized.
    // We use the same workaround as cli_old: wrap in Option and use
    // pending() when None so the arm never fires on non-Unix.
    #[cfg(unix)]
    let mut sigwinch: Option<tokio::signal::unix::Signal> = if interactive {
        Some(tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::window_change(),
        )?)
    } else {
        None
    };

    loop {
        tokio::select! {
            // DATA from stdin: use read_data() which uses readable_mut().await
            // This is the correct high-level Tokio API that properly re-registers
            // the waker on each select! iteration, preventing the terminal freeze.
            data = stdin.read_data() => {
                match data {
                    Some(data) => {
                        let (to_remote, to_local, actions) =
                            input_engine.process_local(&data);

                        if !to_remote.is_empty() {
                            session.send(&to_remote).await?;
                        }

                        // Local echo/erase feedback (e.g. showing `~` when armed,
                        // then erasing it once the escape command is resolved).
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
                                    // Send \r so the remote shell reprints its prompt.
                                    let _ = session.send(b"\r").await;
                                }
                                EscapeAction::CommandPrompt => {
                                    // Mode switch and prompt printing now handled internally by input_engine
                                }
                                EscapeAction::RunLocal(cmd) => {
                                    if !execute_local_command(&mut session, &mut input_engine, &mut stdout, &mut stdin, &mut transfer_context, cmd).await? {
                                        return Ok(());
                                    }
                                }
                                EscapeAction::RequestCompletion => {
                                    let to_local = input_engine.complete_active_line(&mut session, &transfer_context).await;
                                    if !to_local.is_empty() {
                                        stdout.write_all(&to_local).await?;
                                        stdout.flush().await?;
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        session.eof().await?;
                        break;
                    }
                }
            }

            // RESIZE: on Unix via SIGWINCH; arm never fires on Windows.
            _ = async {
                #[cfg(unix)]
                if let Some(s) = sigwinch.as_mut() {
                    s.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
                #[cfg(not(unix))]
                std::future::pending::<()>().await;
            } => {
                let size = current_terminal_size();
                let _ = session.resize(size).await;
            }

            // DATA and RESIZE from the remote session.
            event = session.next_event() => {
                match event? {
                    Some(SessionEvent::Data(data)) => {
                        input_engine.observe_remote(&data);
                        stdout.write_all(&data).await?;
                        stdout.flush().await?;
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
        .write_all(b"(Escape sequences are only recognized at the start of a line)")
        .await?;
    stdout.flush().await?;
    Ok(())
}
