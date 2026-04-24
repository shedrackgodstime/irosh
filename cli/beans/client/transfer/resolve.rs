use anyhow::Result;
use irosh::Session;
use irosh::error::ClientError;

use crate::support::normalize_path;

pub(crate) fn auto_rename_download_target(path: std::path::PathBuf) -> std::path::PathBuf {
    if !path.exists() {
        return path;
    }

    let parent = path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let ext = path.extension().and_then(|s| s.to_str());

    for index in 1.. {
        let candidate_name = match ext {
            Some(ext) => format!("{stem} ({index}).{ext}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("infinite candidate iterator should always find a free path")
}

pub(crate) async fn resolve_remote_source_path(
    session: &Session,
    raw: &str,
) -> Result<std::path::PathBuf> {
    resolve_remote_path(session, Some(raw), None).await
}

pub(crate) async fn resolve_remote_target_path(
    session: &Session,
    raw: Option<&str>,
    fallback_name: &str,
) -> Result<std::path::PathBuf> {
    resolve_remote_path(session, raw, Some(fallback_name)).await
}

async fn resolve_remote_path(
    session: &Session,
    raw: Option<&str>,
    fallback_name: Option<&str>,
) -> Result<std::path::PathBuf> {
    let Some(raw) = raw else {
        let cwd = session.remote_cwd().await?;
        let fallback = fallback_name.ok_or_else(|| ClientError::TransferTargetInvalid {
            reason: "missing fallback file name",
        })?;
        return Ok(normalize_path(cwd.join(fallback)));
    };

    if raw == "." || raw == "./" {
        let cwd = session.remote_cwd().await?;
        let fallback = fallback_name.ok_or_else(|| ClientError::TransferTargetInvalid {
            reason: "missing fallback file name",
        })?;
        return Ok(normalize_path(cwd.join(fallback)));
    }

    let path = std::path::Path::new(raw);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    if raw == "~" {
        return Ok(match fallback_name {
            Some(fallback) => std::path::PathBuf::from(raw).join(fallback),
            None => std::path::PathBuf::from(raw),
        });
    }

    if let Some(home_relative) = raw.strip_prefix("~/") {
        return Ok(match fallback_name {
            Some(fallback) if raw.ends_with('/') => std::path::PathBuf::from("~/")
                .join(home_relative)
                .join(fallback),
            _ => std::path::PathBuf::from(raw),
        });
    }

    let cwd = session.remote_cwd().await?;
    if raw.ends_with('/') {
        let fallback = fallback_name.ok_or_else(|| ClientError::TransferTargetInvalid {
            reason: "missing fallback file name",
        })?;
        Ok(normalize_path(cwd.join(raw).join(fallback)))
    } else {
        Ok(normalize_path(cwd.join(raw)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::{looks_like_directory, resolve_local_input_path};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("irosh-cli-test-{label}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn auto_rename_download_target_finds_next_free_name() {
        let local_root = temp_dir("local-target");
        let existing = local_root.join("report.txt");
        fs::write(&existing, b"existing").unwrap();

        assert_eq!(
            auto_rename_download_target(existing),
            local_root.join("report (1).txt")
        );
    }

    #[test]
    fn directory_detection_respects_existing_directories() {
        let local_root = temp_dir("looks-like-dir");
        let dir = local_root.join("downloads");
        fs::create_dir_all(&dir).unwrap();
        let path = resolve_local_input_path(&local_root, "downloads");

        assert!(looks_like_directory("downloads", &path));
    }
}
