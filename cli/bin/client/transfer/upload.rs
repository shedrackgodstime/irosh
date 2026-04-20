use anyhow::Result;
use irosh::Session;
use irosh::error::ClientError;
use std::io::IsTerminal;
use tokio::io::AsyncWriteExt;

use crate::local::TransferContext;
use crate::support::{best_error_message, display_local_path, display_remote_resolved};
use crate::transfer::resolve_remote_target_path;

pub(crate) async fn handle_put_command(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    transfer_context: &TransferContext,
    local: &str,
    remote: Option<&str>,
) -> Result<()> {
    let local_path = transfer_context.resolve_local_source(local);
    if !local_path.exists() {
        stdout
            .write_all(format!("Local file not found: {}\r\n", local_path.display()).as_bytes())
            .await?;
        stdout.flush().await?;
        return Ok(());
    }

    let local_name = local_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ClientError::TransferTargetInvalid {
            reason: "local path has no file name",
        })?;
    let remote_path = resolve_remote_target_path(session, remote, local_name).await?;
    let local_label = display_local_path(&local_path);
    let remote_label = display_remote_resolved(&remote_path);

    stdout
        .write_all(format!("[local] {local_label} -> [remote] {remote_label}\r\n").as_bytes())
        .await?;
    stdout.flush().await?;
    let interactive_progress = std::io::stdout().is_terminal();
    let mut last_percent = None;
    match session
        .put_file_with_progress(&local_path, &remote_path, |progress| {
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
                .write_all(format!("Uploaded {}\r\n", local_name).as_bytes())
                .await?;
        }
        Err(err) => {
            if interactive_progress {
                stdout.write_all(b"\r").await?;
            }
            stdout
                .write_all(
                    format!(
                        "Upload failed.\r\n[local] {local_label} -> [remote] {remote_label}\r\nError: {}\r\n",
                        best_error_message(&err)
                    )
                    .as_bytes(),
                )
                .await?;
        }
    }

    Ok(())
}
