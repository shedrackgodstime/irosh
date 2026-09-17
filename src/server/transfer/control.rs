//! Transfer control message handling.
use crate::error::{Result, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    CompletionRequest, CompletionResponse, CwdResponse, ExistsRequest, ExistsResponse,
    write_completion_response, write_cwd_response, write_exists_response,
};

use super::ConnectionShellState;
use super::ShellContext;

pub(super) async fn handle_exists_request(
    stream: &mut IrohDuplex,
    request: ExistsRequest,
    context: ShellContext,
    shell_state: &ConnectionShellState,
) -> Result<()> {
    let resolved = context.resolve_path(&request.path, shell_state).await?;
    let path_str = resolved.display().to_string();

    let exists = context.path_exists(&path_str).await?;
    let is_dir = context.is_dir(&path_str).await?;

    write_exists_response(stream, &ExistsResponse { exists, is_dir })
        .await
        .map_err(TransportError::from)?;
    Ok(())
}

pub(super) async fn handle_cwd_request(
    stream: &mut IrohDuplex,
    context: ShellContext,
    shell_state: &ConnectionShellState,
) -> Result<()> {
    let cwd = context.cwd(shell_state).await?;
    write_cwd_response(
        stream,
        &CwdResponse {
            path: cwd.display().to_string(),
        },
    )
    .await
    .map_err(TransportError::from)?;
    Ok(())
}

pub(super) async fn handle_completion_request(
    stream: &mut IrohDuplex,
    request: CompletionRequest,
    context: ShellContext,
    shell_state: &ConnectionShellState,
) -> Result<()> {
    let resolved = context.resolve_path(&request.path, shell_state).await?;

    // Determine the search directory and the prefix
    let (search_dir, prefix) = if request.path.ends_with('/') || (request.path.is_empty()) {
        (resolved.clone(), String::new())
    } else {
        let parent = resolved.parent().unwrap_or(&resolved);
        let name = resolved
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        (parent.to_path_buf(), name)
    };

    let mut matches = Vec::new();

    match context {
        ShellContext::Stateless => {
            if let Ok(mut entries) = tokio::fs::read_dir(&search_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with(&prefix) {
                        let mut match_name = name;
                        if let Ok(meta) = entry.metadata().await {
                            if meta.is_dir() {
                                match_name.push('/');
                            }
                        }
                        matches.push(match_name);
                    }
                }
            }
        }
        ShellContext::Live { .. } => {
            #[cfg(target_os = "linux")]
            {
                // In Live context, use 'find' inside the namespace.
                // GNU find's -printf is Linux-specific; BSD find (macOS) lacks it.
                // Fields are NUL-separated: `%P\0%y\0`. A newline or `:` separator
                // mangles filenames that contain those bytes, and NUL cannot occur
                // in a path. The prefix is glob-escaped so metacharacters in the
                // user's partial name are matched literally.
                let escaped = glob_escape(&prefix).replace('\'', "'\\''");
                let find_script =
                    format!("find . -maxdepth 1 -name '{escaped}*' -printf '%P\\0%y\\0'");
                let mut cmd = tokio::process::Command::new("sh");
                cmd.arg("-c")
                    .arg(find_script)
                    .current_dir(&search_dir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null());
                context.configure(&mut cmd);

                // `output()` collects the (prefix-filtered) listing and reaps the
                // child, so no zombie `find` is left behind on any path.
                match cmd.output().await {
                    Ok(output) if output.status.success() => {
                        matches.extend(parse_find_type_entries(&output.stdout));
                    }
                    Ok(_) => {}
                    Err(e) => tracing::debug!("completion find failed: {e}"),
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                if let Ok(mut entries) = tokio::fs::read_dir(&search_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with(&prefix) {
                            let mut match_name = name;
                            if let Ok(meta) = entry.metadata().await
                                && meta.is_dir()
                            {
                                match_name.push('/');
                            }
                            matches.push(match_name);
                        }
                    }
                }
            }
        }
    }

    matches.sort();
    write_completion_response(stream, &CompletionResponse { matches })
        .await
        .map_err(TransportError::from)?;
    Ok(())
}

/// Escapes glob metacharacters so a completion prefix is matched literally by
/// `find -name`.
#[cfg(target_os = "linux")]
fn glob_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '*' | '?' | '[' | ']' | '\\' => {
                out.push('[');
                out.push(c);
                out.push(']');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Parses NUL-separated `name\0type\0` records emitted by `find -printf`, adding
/// a trailing `/` to directory entries.
#[cfg(target_os = "linux")]
fn parse_find_type_entries(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut fields = bytes.split(|b| *b == 0);
    while let (Some(name), Some(kind)) = (fields.next(), fields.next()) {
        if name.is_empty() {
            continue;
        }
        let mut entry = String::from_utf8_lossy(name).into_owned();
        if kind == b"d" {
            entry.push('/');
        }
        out.push(entry);
    }
    out
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{glob_escape, parse_find_type_entries};

    #[test]
    fn glob_escape_neutralizes_metacharacters() {
        assert_eq!(glob_escape("plain"), "plain");
        assert_eq!(glob_escape("a*b"), "a[*]b");
        assert_eq!(glob_escape("a?b"), "a[?]b");
        assert_eq!(glob_escape("a[b"), "a[[]b");
        assert_eq!(glob_escape("a]b"), "a[]]b");
    }

    #[test]
    fn parse_find_entries_handles_colons_and_spaces() {
        let input = b"weird:name.txt\0f\0with space\0d\0";
        assert_eq!(
            parse_find_type_entries(input),
            vec!["weird:name.txt".to_string(), "with space/".to_string()]
        );
    }

    #[test]
    fn parse_find_entries_is_empty_on_no_output() {
        assert!(parse_find_type_entries(b"").is_empty());
    }
}
