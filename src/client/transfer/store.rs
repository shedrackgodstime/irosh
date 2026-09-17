//! Transfer state/store.
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{ClientError, Result};

pub(super) fn temp_transfer_path(dest: &std::path::Path) -> std::path::PathBuf {
    let parent = dest.parent().unwrap_or(std::path::Path::new("."));
    let file_name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("transfer");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    parent.join(format!(".{file_name}.{unique}.irosh_part"))
}

/// Replaces `final_path` with `temp_path`.
///
/// On Unix a plain `rename` is atomic and replaces an existing destination.
/// On Windows `std::fs::rename` fails when the destination exists, so
/// `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH` is
/// used instead; that keeps the replace atomic and durable rather than falling
/// back to a non-atomic copy.
async fn replace_file(
    temp_path: &std::path::Path,
    final_path: &std::path::Path,
) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let temp_path = temp_path.to_path_buf();
        let final_path = final_path.to_path_buf();
        return tokio::task::spawn_blocking(move || move_file_ex_replace(&temp_path, &final_path))
            .await
            .map_err(std::io::Error::other)?;
    }
    #[cfg(not(windows))]
    tokio::fs::rename(temp_path, final_path).await
}

#[cfg(windows)]
fn move_file_ex_replace(
    temp_path: &std::path::Path,
    final_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = temp_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination: Vec<u16> = final_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: both buffers are valid, NUL-terminated UTF-16 strings that live
    // for the duration of the call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub(super) async fn persist_temp_file(
    temp_path: &std::path::Path,
    final_path: &std::path::Path,
) -> Result<()> {
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| ClientError::FileIo {
                operation: "create destination directory",
                path: parent.to_path_buf(),
                source,
            })?;
    }

    match replace_file(temp_path, final_path).await {
        Ok(()) => Ok(()),
        Err(_rename_err) => {
            tokio::fs::copy(temp_path, final_path)
                .await
                .map_err(|source| ClientError::FileIo {
                    operation: "persist temp file",
                    path: final_path.to_path_buf(),
                    source,
                })?;
            let _ = tokio::fs::remove_file(temp_path).await;
            Ok(())
        }
    }
}
