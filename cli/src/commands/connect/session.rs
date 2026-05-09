use anyhow::Result;
use irosh::sys::{AsyncStdin, TerminalEvent};
use irosh::{Session, SessionEvent};
use std::io::IsTerminal;
use tokio::io::AsyncWriteExt;

pub async fn drive_session(mut session: Session) -> Result<()> {
    let mut stdin = AsyncStdin::new()?;
    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();

    #[allow(unused_variables)]
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    loop {
        tokio::select! {
            event = std::future::poll_fn(|cx| stdin.poll_next(cx)) => {
                match event {
                    Some(TerminalEvent::Data(data)) => {
                        session.send(&data).await?;
                    }
                    Some(TerminalEvent::Resize(size)) => {
                        let _ = session.resize(size).await;
                    }
                    None => {
                        session.eof().await?;
                        break;
                    }
                }
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
