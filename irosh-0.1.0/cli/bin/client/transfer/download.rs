use anyhow::Result;
use irosh::Session;
use std::io::IsTerminal;
use tokio::io::AsyncWriteExt;

use crate::local::TransferContext;
use crate::support::{best_error_message, display_local_path, display_remote_resolved};
use crate::transfer::resolve_remote_source_path;

pub(crate) async fn handle_get_command(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    transfer_context: &TransferContext,
    remote: &str,
    local: Option<&str>,
) -> Result<()> {
    let remote_path = resolve_remote_source_path(session, remote).await?;
    let remote_name = remote_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("downloaded-file");
    let local_path = transfer_context.resolve_local_target(local, remote_name);
    let remote_label = display_remote_resolved(&remote_path);
    let local_label = display_local_path(&local_path);

    stdout
        .write_all(format!("[remote] {remote_label} -> [local] {local_label}\r\n").as_bytes())
        .await?;
    stdout.flush().await?;
    let interactive_progress = std::io::stdout().is_terminal();
    let mut last_percent = None;
    match session
        .get_file_with_progress(&remote_path, &local_path, |progress| {
            if !interactive_progress {
                return;
            }
            let percent = progress.percent();
            if last_percent == Some(percent) {
                return;
            }
            last_percent = Some(percent);
            print!("\rProgress: {percent:>3}%");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        })
        .await
    {
        Ok(()) => {
            if interactive_progress {
                stdout.write_all(b"\rProgress: 100%\r\n").await?;
            }
            stdout
                .write_all(format!("Downloaded {}\r\n", remote_name).as_bytes())
                .await?;
        }
        Err(err) => {
            if interactive_progress {
                stdout.write_all(b"\r").await?;
            }
            stdout
                .write_all(
                    format!(
                        "Download failed.\r\n[remote] {remote_label} -> [local] {local_label}\r\nError: {}\r\n",
                        best_error_message(&err)
                    )
                    .as_bytes(),
                )
                .await?;
        }
    }

    Ok(())
}
