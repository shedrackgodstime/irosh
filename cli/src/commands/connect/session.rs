use anyhow::Result;
use irosh::sys::{AsyncStdin, current_terminal_size};
use irosh::{Session, SessionEvent};
use std::io::IsTerminal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn drive_session(mut session: Session) -> Result<()> {
    let mut stdin = AsyncStdin::new()?;
    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut buf = vec![0u8; 4096];

    #[allow(unused_variables)]
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

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
            res = stdin.read(&mut buf) => {
                match res? {
                    0 => {
                        session.eof().await?;
                        break;
                    }
                    n => {
                        session.send(&buf[..n]).await?;
                    }
                }
            }
            _ = async {
                #[cfg(unix)]
                if let Some(s) = sigwinch.as_mut() { s.recv().await; }
                else { std::future::pending::<()>().await; }
                #[cfg(not(unix))]
                std::future::pending::<()>().await;
            } => {
                let size = current_terminal_size();
                let _ = session.resize(size).await;
            }
            event = session.next_event() => {
                match event? {
                    Some(SessionEvent::Data(data)) => {
                        stdout.write_all(&data).await?;
                        stdout.flush().await?;
                    }
                    Some(SessionEvent::ExtendedData(data, _)) => {
                        stderr.write_all(&data).await?;
                        stderr.flush().await?;
                    }
                    Some(SessionEvent::Closed) => {
                        println!("\r\nSession closed.");
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
