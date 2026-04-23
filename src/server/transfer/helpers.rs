use std::path::Path;

use tokio::process::{Child, Command};

use crate::error::{Result, ServerError};
use crate::transport::transfer::{TransferFailure, TransferFailureCode};

use super::{ShellContext, resolve_remote_path};

pub(super) struct PreparedPutDestination {
    pub(super) final_arg: String,
    pub(super) part_arg: String,
}

pub(super) async fn prepare_put_destination(
    context: ShellContext,
    raw_path: &str,
) -> Result<Option<PreparedPutDestination>> {
    let dest_path = resolve_remote_path(raw_path)?;
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

pub(super) async fn spawn_upload_helper(
    context: ShellContext,
    dest: &str,
) -> Result<tokio::process::Child> {
    let mut upload_cmd = Command::new("sh");
    upload_cmd
        .arg("-c")
        .arg("cat > \"$1\"")
        .arg("sh")
        .arg(dest)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    context.configure(&mut upload_cmd);

    upload_cmd.spawn().map_err(|e| {
        ServerError::TransferFailed {
            details: format!("failed to spawn upload helper: {e}"),
        }
        .into()
    })
}

pub(super) async fn probe_download_size(
    context: ShellContext,
    source_path: &Path,
) -> Result<std::result::Result<u64, TransferFailure>> {
    let helper_source = source_path.display().to_string();

    let mut size_probe_cmd = Command::new("sh");
    size_probe_cmd
        .arg("-c")
        .arg("wc -c < \"$1\"")
        .arg("sh")
        .arg(&helper_source)
        .stderr(std::process::Stdio::piped());
    context.configure(&mut size_probe_cmd);

    let size_probe = size_probe_cmd
        .output()
        .await
        .map_err(|e| ServerError::TransferFailed {
            details: format!("failed to probe download source size: {e}"),
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

    let expected_size = String::from_utf8_lossy(&size_probe.stdout)
        .trim()
        .parse::<u64>()
        .map_err(|e| ServerError::TransferFailed {
            details: format!("failed to parse download source size: {e}"),
        })?;
    Ok(Ok(expected_size))
}

pub(super) async fn spawn_download_helper(
    context: ShellContext,
    source_path: &Path,
) -> Result<(Child, String)> {
    let helper_source = source_path.display().to_string();

    let mut download_cmd = Command::new("cat");
    download_cmd.arg("--").arg(&helper_source);
    download_cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    context.configure(&mut download_cmd);

    let child = download_cmd
        .spawn()
        .map_err(|e| ServerError::TransferFailed {
            details: format!("failed to spawn download helper: {e}"),
        })?;
    Ok((child, helper_source))
}
