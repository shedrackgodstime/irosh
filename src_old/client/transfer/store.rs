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

    match tokio::fs::rename(temp_path, final_path).await {
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
