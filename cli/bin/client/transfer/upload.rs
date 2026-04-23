use anyhow::Result;
use irosh::Session;
use irosh::error::ClientError;
use std::io::IsTerminal;
use tokio::io::AsyncWriteExt;

use crate::local::TransferContext;
use crate::support::{best_error_message, display_local_path, display_remote_resolved};
use crate::transfer::resolve_remote_target_path;
use indicatif::{ProgressBar, ProgressStyle};

pub(crate) async fn handle_put_command<S>(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut S,
    transfer_context: &TransferContext,
    local: &str,
    remote: Option<&str>,
    recursive: bool,
) -> Result<()>
where
    S: tokio::io::AsyncRead + Unpin,
{
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

    let pb = if std::io::stdout().is_terminal() {
        let pb = ProgressBar::new(0);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    use tokio::io::AsyncReadExt;
    let pb_clone = pb.clone();
    let mut cancel_buf = [0u8; 1];

    let transfer_res = tokio::select! {
        res = session.put_with_progress(&local_path, &remote_path, recursive, move |progress| {
            if let Some(pb) = &pb_clone {
                pb.set_length(progress.total);
                pb.set_position(progress.transferred);
            }
        }) => res,
        _ = async {
            loop {
                match stdin.read(&mut cancel_buf).await {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if cancel_buf[..n].contains(&3) { // Ctrl+C
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        } => {
            return Err(anyhow::anyhow!("Transfer cancelled by user (Ctrl+C)"));
        }
    };

    match transfer_res {
        Ok(()) => {
            if let Some(pb) = pb {
                pb.finish_and_clear();
            }
            stdout
                .write_all(format!("Uploaded {}\r\n", local_name).as_bytes())
                .await?;
        }
        Err(err) => {
            if let Some(pb) = pb {
                pb.abandon();
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
