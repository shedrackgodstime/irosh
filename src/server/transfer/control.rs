use crate::error::{Result, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    CompletionRequest, CompletionResponse, CwdResponse, ExistsRequest, ExistsResponse,
    write_completion_response, write_cwd_response, write_exists_response,
};

use super::ShellContext;

pub(super) async fn handle_exists_request(
    stream: &mut IrohDuplex,
    request: ExistsRequest,
    context: ShellContext,
) -> Result<()> {
    let resolved = context.resolve_path(&request.path).await?;
    let path_str = resolved.display().to_string();

    let exists = context.path_exists(&path_str).await?;

    write_exists_response(stream, &ExistsResponse { exists })
        .await
        .map_err(TransportError::from)?;
    Ok(())
}

pub(super) async fn handle_cwd_request(
    stream: &mut IrohDuplex,
    context: ShellContext,
) -> Result<()> {
    let cwd = context.cwd().await?;
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
) -> Result<()> {
    let resolved = context.resolve_path(&request.path).await?;

    // Determine the search directory and the prefix
    let (search_dir, prefix) = if request.path.ends_with('/') || (request.path.is_empty()) {
        (resolved.clone(), "".to_string())
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
            // In Live context, use 'find' inside the namespace
            let mut cmd = tokio::process::Command::new("sh");
            // find . -maxdepth 1 -name 'prefix*' -printf '%P%y\n'
            // %y is type (f, d, etc.)
            let find_script = format!(
                "find . -maxdepth 1 -name '{}*' -printf '%P:%y\\n'",
                prefix.replace("'", "'\\''")
            );
            cmd.arg("-c")
                .arg(find_script)
                .current_dir(&search_dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            context.configure(&mut cmd);

            if let Ok(child) = cmd.spawn() {
                if let Some(stdout) = child.stdout {
                    use tokio::io::{AsyncBufReadExt, BufReader};
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let parts: Vec<&str> = line.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let mut name = parts[0].to_string();
                            if parts[1] == "d" {
                                name.push('/');
                            }
                            matches.push(name);
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
