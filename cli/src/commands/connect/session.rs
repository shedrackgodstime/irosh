use anyhow::Result;
use futures_util::FutureExt;
use irosh::sys::AsyncStdin;
use irosh::{Session, SessionEvent};
use tokio::io::AsyncWriteExt;

use super::input::{EscapeAction, InputEngine, InputMode};
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
            // SINGLE SOURCE OF TRUTH: All local input (Raw or Prompt) comes through here.
            event = stdin.next_event().fuse() => {
                match event {
                    Some(irosh::sys::TerminalEvent::Data(data)) => {
                        let (to_remote, to_local, actions) =
                            input_engine.process_local(&data);

                        if !to_remote.is_empty() {
                            session.send(&to_remote).await?;
                        }

                        if !to_local.is_empty() {
                            stdout.write_all(&to_local).await?;
                            stdout.flush().await?;
                        }

                        let mut actions_queue = actions;
                        while !actions_queue.is_empty() {
                            let action = actions_queue.remove(0);
                            let (cont, swallowed) = handle_action(action, &mut session, &mut input_engine, &mut stdout, &mut stdin, &mut transfer_context, &mut remote_buffer).await?;

                            if !swallowed.is_empty() {
                                let (to_remote, to_local, mut extra_actions) = input_engine.process_local(&swallowed);
                                if !to_remote.is_empty() {
                                    session.send(&to_remote).await?;
                                }
                                if !to_local.is_empty() {
                                    stdout.write_all(&to_local).await?;
                                    stdout.flush().await?;
                                }
                                actions_queue.append(&mut extra_actions);
                            }

                            if !cont {
                                return Ok(());
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

async fn handle_action(
    action: EscapeAction,
    session: &mut Session,
    input_engine: &mut InputEngine,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut AsyncStdin,
    transfer_context: &mut TransferContext,
    remote_buffer: &mut Vec<u8>,
) -> Result<(bool, Vec<u8>)> {
    let mut lines_printed: u16 = 0;
    let mut before_row: u16 = 0;
    let mut swallowed = Vec::new();

    if input_engine.remote_is_windows {
        let (r, extra) = query_cpr(stdout, stdin).await.unwrap_or((0, Vec::new()));
        before_row = r;
        swallowed = extra;
    }
    match action {
        EscapeAction::Disconnect => {
            stdout.write_all(b"[irosh] Disconnecting...\r\n").await?;
            stdout.flush().await?;
            return Ok((false, swallowed));
        }
        EscapeAction::Help => {
            show_help(stdout).await?;
            let _ = stdout.write_all(b"\r\n").await;
            let _ = stdout.flush().await;
            lines_printed = 11;

            if input_engine.mode == InputMode::Remote && !remote_buffer.is_empty() {
                stdout.write_all(remote_buffer).await?;
                stdout.flush().await?;
                remote_buffer.clear();
            }
        }
        EscapeAction::CommandPrompt => {
            // Prompt initialization is handled inside InputEngine
        }
        EscapeAction::RunLocal(cmd) => {
            let (cont, lp) =
                execute_local_command(session, input_engine, stdout, stdin, transfer_context, cmd)
                    .await?;
            lines_printed = lp;
            if !cont {
                return Ok((false, swallowed));
            }

            if input_engine.mode == InputMode::Remote && !remote_buffer.is_empty() {
                stdout.write_all(remote_buffer).await?;
                stdout.flush().await?;
                remote_buffer.clear();
            }
        }
        EscapeAction::RequestCompletion => {
            let feedback = input_engine
                .complete_active_line(session, transfer_context)
                .await;
            if !feedback.is_empty() {
                stdout.write_all(&feedback).await?;
                stdout.flush().await?;
            }
        }
    }

    if input_engine.remote_is_windows && lines_printed > 0 {
        let size = irosh::sys::current_terminal_size();
        let s = (before_row.saturating_add(lines_printed)).saturating_sub(size.rows as u16);
        if s > 0 {
            session.send(b"\x0C\r").await?;
        }
    }

    Ok((true, swallowed))
}

async fn query_cpr(
    stdout: &mut tokio::io::Stdout,
    stdin: &mut AsyncStdin,
) -> Result<(u16, Vec<u8>)> {
    stdout.write_all(b"\x1b[6n").await?;
    stdout.flush().await?;

    let mut response = Vec::new();
    let timeout = tokio::time::sleep(std::time::Duration::from_millis(50));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                break;
            }
            event = stdin.next_event() => {
                if let Some(irosh::sys::TerminalEvent::Data(data)) = event {
                    response.extend_from_slice(&data);
                    if response.ends_with(b"R") {
                        break;
                    }
                }
            }
        }
    }

    let mut row = 0;
    let mut extra = Vec::new();

    if let Some(start) = response.windows(2).position(|w| w == b"\x1b[") {
        if let Some(end_offset) = response[start..].iter().position(|&b| b == b'R') {
            let end = start + end_offset;
            extra.extend_from_slice(&response[..start]);
            extra.extend_from_slice(&response[end + 1..]);

            let s = String::from_utf8_lossy(&response[start + 2..end]);
            if let Some(row_str) = s.split(';').next() {
                if let Ok(r) = row_str.parse::<u16>() {
                    row = r;
                }
            }
        } else {
            extra = response;
        }
    } else {
        extra = response;
    }

    Ok((row, extra))
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
