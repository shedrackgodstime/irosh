use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

use crate::error::{Result, ServerError};
use crate::transport::transfer::{TransferFailure, TransferFailureCode};

use super::ShellContext;

pub(super) struct PreparedPutDestination {
    pub(super) final_arg: String,
    pub(super) part_arg: String,
}

pub(super) async fn prepare_put_destination(
    context: ShellContext,
    raw_path: &str,
) -> Result<Option<PreparedPutDestination>> {
    let dest_path = context.resolve_path(raw_path).await?;
    let final_arg = dest_path.display().to_string();

    if !context.path_missing(&final_arg).await? {
        return Ok(None);
    }

    if !context
        .create_dir_all(dest_path.parent().unwrap_or(Path::new(".")))
        .await?
    {
        return Err(ServerError::TransferFailed {
            details: format!(
                "failed to create destination directory: {}",
                dest_path.parent().unwrap_or(Path::new(".")).display()
            ),
        }
        .into());
    }

    let mut part_path = dest_path.clone();
    let part_name = format!(
        ".{}.irosh_part",
        dest_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("transfer")
    );
    part_path.set_file_name(part_name);

    Ok(Some(PreparedPutDestination {
        final_arg,
        part_arg: part_path.display().to_string(),
    }))
}

pub(super) fn target_exists_failure(path: &Path) -> TransferFailure {
    TransferFailure::new(
        TransferFailureCode::TargetAlreadyExists,
        path.display().to_string(),
    )
}

pub(super) fn atomic_rename_failure(path: &str) -> TransferFailure {
    TransferFailure::new(TransferFailureCode::AtomicRenameFailed, path.to_string())
}

pub(super) enum UploadSink {
    Process(tokio::process::Child),
    File(tokio::fs::File),
}

impl UploadSink {
    pub(super) fn stdin(&mut self) -> Option<Box<dyn tokio::io::AsyncWrite + Unpin + Send + '_>> {
        match self {
            Self::Process(child) => child
                .stdin
                .as_mut()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncWrite + Unpin + Send>),
            Self::File(file) => Some(Box::new(file) as Box<dyn tokio::io::AsyncWrite + Unpin + Send>),
        }
    }

    pub(super) async fn wait(self) -> Result<()> {
        match self {
            Self::Process(child) => {
                let output = child.wait_with_output().await.map_err(|e| {
                    ServerError::TransferFailed {
                        details: format!("waiting for upload helper failed: {e}"),
                    }
                })?;
                if !output.status.success() {
                    return Err(ServerError::TransferFailed {
                        details: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                    }
                    .into());
                }
                Ok(())
            }
            Self::File(mut file) => {
                file.flush().await.map_err(|e| ServerError::TransferFailed {
                    details: format!("failed to flush upload file: {e}"),
                })?;
                Ok(())
            }
        }
    }
}

pub(super) async fn spawn_upload_helper(
    _context: ShellContext,
    dest: &str,
) -> Result<UploadSink> {
    #[cfg(target_os = "linux")]
    if let ShellContext::Live { .. } = context {
        let mut upload_cmd = Command::new("sh");
        upload_cmd
            .arg("-c")
            .arg("cat > \"$1\"")
            .arg("sh")
            .arg(dest)
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        context.configure(&mut upload_cmd);

        return Ok(UploadSink::Process(upload_cmd.spawn().map_err(|e| {
            ServerError::TransferFailed {
                details: format!("failed to spawn upload helper: {e}"),
            }
        })?));
    }

    let file = tokio::fs::File::create(dest).await.map_err(|e| {
        ServerError::TransferFailed {
            details: format!("failed to create upload file: {e}"),
        }
    })?;
    Ok(UploadSink::File(file))
}

pub(super) async fn probe_download_size(
    _context: ShellContext,
    source_path: &Path,
) -> Result<std::result::Result<u64, TransferFailure>> {
    #[cfg(target_os = "linux")]
    if let ShellContext::Live { .. } = context {
        let helper_source = source_path.display().to_string();
        let mut size_probe_cmd = Command::new("sh");
        size_probe_cmd
            .arg("-c")
            .arg("wc -c < \"$1\"")
            .arg("sh")
            .arg(&helper_source)
            .stderr(std::process::Stdio::piped());
        context.configure(&mut size_probe_cmd);

        let size_probe = size_probe_cmd.output().await.map_err(|e| {
            ServerError::TransferFailed {
                details: format!("failed to probe download source size: {e}"),
            }
        })?;
        if !size_probe.status.success() {
            return Ok(Err(TransferFailure::new(
                TransferFailureCode::HelperFailed,
                format!(
                    "preflight failed: {}; context={:?}; requested={}; helper_arg={}",
                    String::from_utf8_lossy(&size_probe.stderr).trim(),
                    context,
                    source_path.display(),
                    helper_source
                ),
            )));
        }

        let raw_stdout = String::from_utf8_lossy(&size_probe.stdout);
        let cleaned: String = raw_stdout.chars().filter(|c| c.is_ascii_digit()).collect();
        let expected_size = cleaned.parse::<u64>().map_err(|e| {
            ServerError::TransferFailed {
                details: format!("failed to parse download source size: {e}"),
            }
        })?;
        return Ok(Ok(expected_size));
    }

    let metadata = tokio::fs::metadata(source_path).await.map_err(|e| {
        ServerError::TransferFailed {
            details: format!("failed to read download source metadata: {e}"),
        }
    })?;
    Ok(Ok(metadata.len()))
}

pub(super) enum DownloadSource {
    Process(tokio::process::Child),
    File(tokio::fs::File),
}

impl DownloadSource {
    pub(super) fn stdout(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Unpin + Send + '_>> {
        match self {
            Self::Process(child) => child
                .stdout
                .as_mut()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            Self::File(file) => Some(Box::new(file) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        }
    }
}

pub(super) async fn spawn_download_helper(
    _context: ShellContext,
    source_path: &Path,
) -> Result<(DownloadSource, String)> {
    let helper_source = source_path.display().to_string();

    #[cfg(target_os = "linux")]
    if let ShellContext::Live { .. } = context {
        let mut download_cmd = Command::new("cat");
        download_cmd.arg("--").arg(&helper_source);
        download_cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        context.configure(&mut download_cmd);

        let child = download_cmd.spawn().map_err(|e| ServerError::TransferFailed {
            details: format!("failed to spawn download helper: {e}"),
        })?;
        return Ok((DownloadSource::Process(child), helper_source));
    }

    let file = tokio::fs::File::open(source_path).await.map_err(|e| {
        ServerError::TransferFailed {
            details: format!("failed to open download file: {e}"),
        }
    })?;
    Ok((DownloadSource::File(file), helper_source))
}
