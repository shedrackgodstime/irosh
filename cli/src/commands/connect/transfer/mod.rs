mod download;
mod resolve;
mod upload;

use anyhow::Result;
use irosh::Session;
use tokio::io::AsyncWriteExt;

use crate::commands::connect::support::{looks_like_directory, resolve_local_input_path};

pub(super) use download::handle_get_command;
pub(super) use resolve::{
    auto_rename_download_target, resolve_remote_source_path, resolve_remote_target_path,
};
pub(super) use upload::handle_put_command;

#[derive(Debug, Clone)]
pub(super) struct TransferContext {
    pub(super) local_root: std::path::PathBuf,
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

pub(super) async fn run_escape_transfer_command<S>(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut S,
    transfer_context: &TransferContext,
    command: &str,
) -> Result<()>
where
    S: tokio::io::AsyncRead + Unpin,
{
    let parts = match shell_words::split(command.trim()) {
        Ok(parts) => parts,
        Err(err) => {
            print_local_transfer_block(stdout, &format!("Invalid transfer command: {err}")).await?;
            return Ok(());
        }
    };

    let Some(keyword) = parts.first().map(String::as_str) else {
        return Ok(());
    };

    match keyword {
        "~put" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if arg == "-r" || arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(arg.as_str());
                }
            }

            let Some(local) = args.first().copied() else {
                print_local_transfer_block(stdout, "Usage: ~put [-r] <local> [remote]").await?;
                return Ok(());
            };

            let remote = args.get(1).copied();
            handle_put_command(
                session,
                stdout,
                stdin,
                transfer_context,
                local,
                remote,
                recursive,
            )
            .await
        }
        "~get" => {
            let mut recursive = false;
            let mut args = Vec::new();
            for arg in &parts[1..] {
                if arg == "-r" || arg == "--recursive" {
                    recursive = true;
                } else {
                    args.push(arg.as_str());
                }
            }

            let Some(remote) = args.first().copied() else {
                print_local_transfer_block(stdout, "Usage: ~get [-r] <remote> [local]").await?;
                return Ok(());
            };

            let local = args.get(1).copied();
            handle_get_command(
                session,
                stdout,
                stdin,
                transfer_context,
                remote,
                local,
                recursive,
            )
            .await
        }
        _ => Ok(()),
    }
}

async fn print_local_transfer_block(stdout: &mut tokio::io::Stdout, body: &str) -> Result<()> {
    stdout.write_all(b"\r\n").await?;
    stdout
        .write_all(body.replace('\n', "\r\n").as_bytes())
        .await?;
    stdout.flush().await?;
    Ok(())
}
