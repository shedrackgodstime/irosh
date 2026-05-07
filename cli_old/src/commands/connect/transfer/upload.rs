use anyhow::Result;
use irosh::Session;
use irosh::error::ClientError;
use std::io::IsTerminal;
use tokio::io::AsyncWriteExt;

use crate::commands::connect::input::display::print_local_block;
use crate::commands::connect::support::{
    best_error_message, display_local_path, display_remote_resolved,
};
use crate::commands::connect::transfer::{TransferContext, resolve_remote_target_path};
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
        print_local_block(
            stdout,
            &format!("Local file not found: {}", local_path.display()),
        )
        .await?;
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
        .write_all(format!("\r\n[local] {local_label} -> [remote] {remote_label}\r\n").as_bytes())
        .await?;
    stdout.flush().await?;

    let pb = if std::io::stdout().is_terminal() {
        let pb = ProgressBar::new(0);
        let style = if recursive {
            ProgressStyle::default_bar().template(
                "{spinner:.green} [{elapsed_precise}] {bytes} transferred ({bytes_per_sec})",
            )?
        } else {
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {percent}% ({bytes_per_sec}, {eta})")?
                .progress_chars("#>-")
        };
        pb.set_style(style);
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
                if progress.total > 0 {
                    pb.set_length(progress.total);
                }
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
                pb.finish_and_clear();
            }
            let msg = best_error_message(&err);
            if msg.contains("recursive flag not set") {
                stdout
                    .write_all(
                        format!(
                            "Upload failed.\r\nError: '{}' is a directory (use -r for recursive)\r\n",
                            local_name
                        )
                        .as_bytes(),
                    )
                    .await?;
            } else if msg.contains("not found") {
                stdout
                    .write_all(
                        format!(
                            "Upload failed.\r\nError: '{}' not found on local\r\n",
                            local_name
                        )
                        .as_bytes(),
                    )
                    .await?;
            } else {
                stdout
                    .write_all(format!("Upload failed.\r\nError: {}\r\n", msg).as_bytes())
                    .await?;
            }
        }
    }

    Ok(())
}
