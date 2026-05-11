use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use irosh::Session;
use irosh::error::ClientError;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// Context for file transfers, keeping track of the local working directory.
#[derive(Debug, Clone)]
pub struct TransferContext {
    pub local_root: PathBuf,
}

impl TransferContext {
    pub fn new() -> Self {
        Self {
            local_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn resolve_local_source(&self, raw: &str) -> PathBuf {
        resolve_local_input_path(&self.local_root, raw)
    }

    pub fn resolve_local_target(&self, raw: Option<&str>, fallback_name: &str) -> PathBuf {
        match raw {
            None => self.local_root.join(fallback_name),
            Some(raw) if raw == "." || raw == "./" => self.local_root.join(fallback_name),
            Some(raw) => {
                let path = resolve_local_input_path(&self.local_root, raw);
                let is_explicit_dir = raw.ends_with('/') || raw.ends_with('\\');
                if is_explicit_dir || path.is_dir() {
                    auto_rename_download_target(path.join(fallback_name))
                } else {
                    path
                }
            }
        }
    }
}

pub fn resolve_local_input_path(base: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if raw == "~" || raw.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            if raw == "~" {
                return home;
            } else {
                return home.join(raw.strip_prefix("~/").unwrap());
            }
        }
    }
    base.join(path)
}

fn auto_rename_download_target(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
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

pub async fn resolve_remote_source_path(session: &mut Session, raw: &str) -> Result<PathBuf> {
    resolve_remote_path(session, Some(raw), None).await
}

pub async fn resolve_remote_target_path(
    session: &mut Session,
    raw: Option<&str>,
    fallback_name: &str,
) -> Result<PathBuf> {
    resolve_remote_path(session, raw, Some(fallback_name)).await
}

async fn resolve_remote_path(
    session: &mut Session,
    raw: Option<&str>,
    fallback_name: Option<&str>,
) -> Result<PathBuf> {
    let Some(raw) = raw else {
        let cwd = session.remote_cwd().await?;
        let fallback = fallback_name.ok_or_else(|| ClientError::TransferTargetInvalid {
            reason: "missing fallback file name",
        })?;
        return Ok(cwd.join(fallback));
    };

    if raw == "." || raw == "./" {
        let cwd = session.remote_cwd().await?;
        return Ok(match fallback_name {
            Some(fallback) => cwd.join(fallback),
            None => cwd,
        });
    }

    let path = Path::new(raw);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    if raw == "~" {
        return Ok(match fallback_name {
            Some(fallback) => PathBuf::from(raw).join(fallback),
            None => PathBuf::from(raw),
        });
    }

    if let Some(home_relative) = raw.strip_prefix("~/") {
        return Ok(match fallback_name {
            Some(fallback) if raw.ends_with('/') => {
                PathBuf::from("~/").join(home_relative).join(fallback)
            }
            _ => PathBuf::from(raw),
        });
    }

    let cwd = tokio::time::timeout(std::time::Duration::from_secs(10), session.remote_cwd())
        .await
        .map_err(|_| {
            anyhow::anyhow!("Remote path resolution timed out after 10 seconds. The server might be unresponsive.")
        })??;

    let is_windows = session
        .remote_metadata()
        .map(|m| m.os.to_lowercase().contains("windows"))
        .unwrap_or(false);

    // OS-aware path joiner for remote paths
    let remote_join = |base: &Path, parts: &str, is_win: bool| -> PathBuf {
        let base_s = base.to_string_lossy().to_string();
        let sep = if is_win { "\\" } else { "/" };
        let other_sep = if is_win { "/" } else { "\\" };

        let mut joined = base_s;
        if !joined.ends_with(sep) && !joined.ends_with(other_sep) {
            joined.push_str(sep);
        }

        // Replace any "wrong" separators in the input parts
        let normalized_parts = parts.replace(other_sep, sep);
        let trimmed_parts = normalized_parts.trim_start_matches(sep);

        joined.push_str(trimmed_parts);
        PathBuf::from(joined)
    };

    let is_explicit_dir = raw.ends_with('/') || raw.ends_with('\\');
    if is_explicit_dir {
        Ok(match fallback_name {
            Some(fallback) => {
                let base = remote_join(&cwd, raw, is_windows);
                remote_join(&base, fallback, is_windows)
            }
            None => remote_join(&cwd, raw, is_windows),
        })
    } else {
        let full = remote_join(&cwd, raw, is_windows);
        Ok(full)
    }
}

pub fn portable_file_name(path: &Path) -> Option<&str> {
    let s = path.to_str()?;
    // Find the last slash or backslash
    let last_slash = s.rfind('/').unwrap_or(0);
    let last_backslash = s.rfind('\\').unwrap_or(0);
    let last_sep = std::cmp::max(last_slash, last_backslash);

    if last_sep == 0 {
        if s.starts_with('/') || s.starts_with('\\') {
            Some(&s[1..])
        } else {
            Some(s)
        }
    } else {
        Some(&s[last_sep + 1..])
    }
}

pub async fn handle_put_command(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut irosh::sys::AsyncStdin,
    transfer_context: &TransferContext,
    local: &str,
    remote: Option<&str>,
    recursive: bool,
) -> Result<()> {
    let local_path = transfer_context.resolve_local_source(local);
    if !local_path.exists() {
        stdout
            .write_all(format!("Local file not found: {}", local_path.display()).as_bytes())
            .await?;
        return Ok(());
    }

    let local_name = portable_file_name(&local_path)
        .ok_or_else(|| anyhow::anyhow!("local path has no file name"))?;
    let remote_path = resolve_remote_target_path(session, remote, local_name).await?;

    stdout
        .write_all(
            format!(
                "[local] {} -> [remote] {}\r\n",
                local_path.display(),
                remote_path.display()
            )
            .as_bytes(),
        )
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

    let pb_clone = pb.clone();

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
            // Poll stdin to watch for Ctrl+C cancellation; EOF also terminates.
            while let Some(data) = stdin.read_data().await {
                if data.contains(&0x03) { break; } // Ctrl+C
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

            use irosh::error::{ClientError, IroshError};
            let mut handled = false;

            if let IroshError::Client(client_err) = &err {
                match client_err {
                    ClientError::TransferTargetInvalid { reason }
                        if reason.contains("recursive flag not set") =>
                    {
                        stdout
                            .write_all(
                                format!(
                                    "Upload failed. Error: '{}' is a directory (use -r for recursive)\r\n",
                                    local_name
                                )
                                .as_bytes(),
                            )
                            .await?;
                        handled = true;
                    }
                    ClientError::TransferRejected { failure }
                        if failure.code
                            == irosh::transport::transfer::TransferFailureCode::IsDirectory =>
                    {
                        stdout
                            .write_all(
                                format!(
                                    "Upload failed. Error: '{}' is a directory on remote (use -r for recursive)\r\n",
                                    remote_path.display()
                                )
                                .as_bytes(),
                            )
                            .await?;
                        handled = true;
                    }
                    ClientError::FileIo { source, .. }
                        if source.kind() == std::io::ErrorKind::NotFound =>
                    {
                        stdout
                            .write_all(
                                format!(
                                    "Upload failed. Error: '{}' not found on local\r\n",
                                    local_name
                                )
                                .as_bytes(),
                            )
                            .await?;
                        handled = true;
                    }
                    _ => {}
                }
            }

            if !handled {
                let msg = format!("{:#}", err);
                stdout
                    .write_all(format!("Upload failed. Error: {}\r\n", msg).as_bytes())
                    .await?;
            }
        }
    }

    Ok(())
}

pub async fn handle_get_command(
    session: &mut Session,
    stdout: &mut tokio::io::Stdout,
    stdin: &mut irosh::sys::AsyncStdin,
    transfer_context: &TransferContext,
    remote: &str,
    local: Option<&str>,
    recursive: bool,
) -> Result<()> {
    let remote_path = resolve_remote_source_path(session, remote).await?;
    let remote_name = portable_file_name(&remote_path).unwrap_or("downloaded-file");
    let local_path = transfer_context.resolve_local_target(local, remote_name);

    stdout
        .write_all(
            format!(
                "[remote] {} -> [local] {}\r\n",
                remote_path.display(),
                local_path.display()
            )
            .as_bytes(),
        )
        .await?;
    stdout.flush().await?;

    let pb = if std::io::stdout().is_terminal() {
        let pb = ProgressBar::new(0);
        let style = if recursive {
            ProgressStyle::default_bar().template(
                "{spinner:.green} [{elapsed_precise}] {bytes} received ({bytes_per_sec})",
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

    let pb_clone = pb.clone();

    let transfer_res = tokio::select! {
        res = session.get_with_progress(&remote_path, &local_path, recursive, move |progress| {
            if let Some(pb) = &pb_clone {
                if progress.total > 0 {
                    pb.set_length(progress.total);
                }
                pb.set_position(progress.transferred);
            }
        }) => res,
        _ = async {
            // Poll stdin to watch for Ctrl+C cancellation; EOF also terminates.
            while let Some(data) = stdin.read_data().await {
                if data.contains(&0x03) { break; } // Ctrl+C
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
                .write_all(format!("Downloaded {}\r\n", remote_name).as_bytes())
                .await?;
        }
        Err(err) => {
            if let Some(pb) = pb {
                pb.finish_and_clear();
            }

            use irosh::error::{ClientError, IroshError};
            let mut handled = false;

            if let IroshError::Client(client_err) = &err {
                match client_err {
                    ClientError::TransferTargetInvalid { reason }
                        if reason.contains("recursive flag not set") =>
                    {
                        stdout
                            .write_all(
                                format!(
                                    "Download failed. Error: '{}' is a directory (use -r for recursive)\r\n",
                                    remote_name
                                )
                                .as_bytes(),
                            )
                            .await?;
                        handled = true;
                    }
                    ClientError::TransferRejected { failure }
                        if failure.code
                            == irosh::transport::transfer::TransferFailureCode::IsDirectory =>
                    {
                        stdout
                            .write_all(
                                format!(
                                    "Download failed. Error: '{}' is a directory on remote (use -r for recursive)\r\n",
                                    remote_name
                                )
                                .as_bytes(),
                            )
                            .await?;
                        handled = true;
                    }
                    ClientError::TransferRejected { failure }
                        if failure.code
                            == irosh::transport::transfer::TransferFailureCode::NotFound =>
                    {
                        stdout
                            .write_all(
                                format!(
                                    "Download failed. Error: '{}' not found on remote\r\n",
                                    remote_name
                                )
                                .as_bytes(),
                            )
                            .await?;
                        handled = true;
                    }
                    _ => {}
                }
            }

            if !handled {
                let msg = format!("{:#}", err);
                stdout
                    .write_all(format!("Download failed. Error: {}\r\n", msg).as_bytes())
                    .await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("irosh-cli-test-{label}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn test_auto_rename_download_target_finds_next_free_name() {
        let local_root = temp_dir("local-target");
        let existing = local_root.join("report.txt");
        fs::write(&existing, b"existing").unwrap();

        assert_eq!(
            auto_rename_download_target(existing),
            local_root.join("report (1).txt")
        );
    }

    #[test]
    fn test_resolve_local_input_path_absolute() {
        let base = PathBuf::from("/base/dir");
        let path = resolve_local_input_path(&base, "/absolute/path");
        assert_eq!(path, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_resolve_local_input_path_relative() {
        let base = PathBuf::from("/base/dir");
        let path = resolve_local_input_path(&base, "relative/path");
        assert_eq!(path, PathBuf::from("/base/dir/relative/path"));
    }

    #[test]
    fn test_transfer_context_resolves_local_target_directory() {
        let local_root = temp_dir("looks-like-dir");
        let dir = local_root.join("downloads");
        fs::create_dir_all(&dir).unwrap();

        let ctx = TransferContext {
            local_root: local_root.clone(),
        };
        let resolved = ctx.resolve_local_target(Some("downloads"), "file.txt");

        assert_eq!(resolved, dir.join("file.txt"));
    }
}
