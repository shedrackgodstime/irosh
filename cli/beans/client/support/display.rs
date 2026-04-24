use anyhow::{Context, Result};
use tracing_subscriber::{EnvFilter, reload};

use super::local_home_dir;

pub(crate) fn display_remote_resolved(path: &std::path::Path) -> String {
    if matches!(path.to_str(), Some("~")) {
        "~".to_string()
    } else if path.to_str().is_some_and(|raw| raw.starts_with("~/")) {
        path.display().to_string()
    } else if path.as_os_str().is_empty() {
        "~".to_string()
    } else if path.is_absolute() {
        path.display().to_string()
    } else {
        format!("~/{}", path.display())
    }
}

pub(crate) fn display_local_path(path: &std::path::Path) -> String {
    let Some(home) = local_home_dir() else {
        return path.display().to_string();
    };
    if let Ok(suffix) = path.strip_prefix(&home) {
        if suffix.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", suffix.display())
        }
    } else {
        path.display().to_string()
    }
}

pub(crate) fn best_error_message(err: &dyn std::error::Error) -> String {
    let mut best = None;
    let mut current: Option<&dyn std::error::Error> = Some(err);

    while let Some(cause) = current {
        let message = cause.to_string();
        if matches!(
            message.as_str(),
            "client error" | "server error" | "transport error" | "storage error"
        ) {
            current = cause.source();
            continue;
        }
        best = Some(message);
        current = cause.source();
    }

    best.unwrap_or_else(|| err.to_string())
}

pub(crate) fn format_local_listing(path: &std::path::Path) -> Result<String> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;

    if metadata.is_file() {
        return Ok(format!("{}\r\n", path.display()));
    }

    let mut entries = std::fs::read_dir(path)
        .with_context(|| format!("reading directory {}", path.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("collecting directory entries for {}", path.display()))?;

    entries.sort_by_key(|entry| entry.file_name());

    let mut output = String::new();
    for entry in entries {
        let entry_path = entry.path();
        let metadata = entry
            .metadata()
            .with_context(|| format!("reading metadata for {}", entry_path.display()))?;
        let mut name = entry.file_name().to_string_lossy().to_string();
        if metadata.is_dir() {
            name.push('/');
        }
        output.push_str(&name);
        output.push_str("\r\n");
    }

    Ok(output)
}

pub(crate) fn suppress_interactive_logs<S>(handle: &reload::Handle<EnvFilter, S>) {
    let _ = handle.modify(|filter| {
        *filter = EnvFilter::new("off");
    });
}

pub(crate) fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut iter = input.chars().peekable();

    while let Some(c) = iter.next() {
        if c == '\x1b' {
            if let Some('[') = iter.peek() {
                iter.next();
                while let Some(&nc) = iter.peek() {
                    iter.next();
                    if nc == '~' || nc.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        output.push(c);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use irosh::error::ClientError;

    #[test]
    fn best_error_message_prefers_non_generic_causes() {
        use irosh::IroshError;
        let err = anyhow::Error::new(IroshError::Client(ClientError::MetadataFailed {
            detail: "download rejected".to_string(),
        }));

        assert_eq!(
            best_error_message(err.as_ref()),
            "metadata request failed: download rejected"
        );
    }
}
