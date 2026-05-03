pub(crate) fn normalize_path(path: std::path::PathBuf) -> std::path::PathBuf {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

pub(crate) fn resolve_local_input_path(base: &std::path::Path, raw: &str) -> std::path::PathBuf {
    if raw == "~" {
        return local_home_dir().unwrap_or_else(|| base.to_path_buf());
    }

    if let Some(home_relative) = raw.strip_prefix("~/") {
        return local_home_dir()
            .unwrap_or_else(|| base.to_path_buf())
            .join(home_relative);
    }

    let path = std::path::Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

pub(crate) fn looks_like_directory(raw: &str, path: &std::path::Path) -> bool {
    raw.ends_with('/') || path.is_dir()
}

pub(crate) fn local_home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn resolve_local_input_path_expands_home_relative_inputs() {
        let base = temp_dir("local-input");
        let expected_home = local_home_dir().unwrap();

        assert_eq!(resolve_local_input_path(&base, "~"), expected_home);
        assert_eq!(
            resolve_local_input_path(&base, "~/Downloads/file.txt"),
            expected_home.join("Downloads/file.txt")
        );
        assert_eq!(
            resolve_local_input_path(&base, "notes.txt"),
            base.join("notes.txt")
        );
    }

    #[test]
    fn normalize_path_collapses_current_and_parent_components() {
        let path = std::path::PathBuf::from("/tmp/demo/./nested/../file.txt");
        assert_eq!(
            normalize_path(path),
            std::path::PathBuf::from("/tmp/demo/file.txt")
        );
    }
}
