//! Platform shell discovery and remote command construction.

use portable_pty::CommandBuilder;

pub(super) fn build_command(command: Option<&str>) -> CommandBuilder {
    if let Some(command) = command {
        #[cfg(unix)]
        {
            let mut command_builder = CommandBuilder::new("sh");
            command_builder.arg("-lc");
            command_builder.arg(command);
            command_builder
        }
        #[cfg(windows)]
        {
            let exe = windows_command_processor();
            let is_powershell = windows_shell_self_echoes();
            let flag = if is_powershell { "-Command" } else { "/C" };

            // Enforce UTF-8 encoding for the remote session to ensure compatibility with irosh output
            let final_command = if is_powershell {
                format!(
                    "$OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {command}"
                )
            } else {
                format!("chcp 65001 >nul && {command}")
            };

            let mut command_builder = CommandBuilder::new(exe);
            command_builder.arg(flag);
            command_builder.arg(final_command);
            command_builder
        }
        #[cfg(not(any(unix, windows)))]
        {
            let mut command_builder = CommandBuilder::new("sh");
            command_builder.arg("-c");
            command_builder.arg(command);
            command_builder
        }
    } else {
        #[cfg(windows)]
        {
            let exe = windows_command_processor();
            let is_powershell = windows_shell_self_echoes();

            let mut builder = CommandBuilder::new(exe);
            if is_powershell {
                // For PowerShell, we set the output encoding globally for the session.
                builder.arg("-NoExit");
                builder.arg("-Command");
                builder.arg("$OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8;");
            } else {
                // For CMD, we use /K to run chcp and stay open.
                builder.arg("/K");
                builder.arg("chcp 65001 >nul");
            }
            builder
        }
        #[cfg(not(windows))]
        {
            CommandBuilder::new_default_prog()
        }
    }
}

#[cfg(windows)]
fn windows_command_processor() -> String {
    use std::sync::OnceLock;
    static SHELL: OnceLock<String> = OnceLock::new();
    SHELL.get_or_init(detect_windows_shell).clone()
}

/// True when the configured Windows shell renders typed input itself
/// (PowerShell/PSReadLine). The server must not echo in that case or every
/// keystroke would appear twice. cmd.exe has no line editor, so it relies
/// on the peer to echo.
#[cfg(windows)]
pub(super) fn windows_shell_self_echoes() -> bool {
    let exe = windows_command_processor().to_lowercase();
    exe.contains("powershell") || exe.contains("pwsh")
}

#[cfg(windows)]
fn detect_windows_shell() -> String {
    use std::path::Path;

    // 1. Try to find PowerShell Core (pwsh.exe) in PATH
    if let Ok(output) = std::process::Command::new("where.exe")
        .arg("pwsh.exe")
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            if !path.is_empty() && Path::new(&path).exists() {
                return path;
            }
        }
    }

    // 2. Try to find Windows PowerShell in standard location
    if let Ok(systemroot) = std::env::var("SystemRoot") {
        let ps_path = std::path::PathBuf::from(&systemroot)
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if ps_path.exists() {
            return ps_path.to_string_lossy().into_owned();
        }
    }

    // 3. Fallback to COMSPEC or cmd.exe
    if let Ok(comspec) = std::env::var("COMSPEC") {
        if Path::new(&comspec).is_absolute() && Path::new(&comspec).exists() {
            return comspec;
        }
    }

    // Absolute fallback
    if let Ok(systemroot) = std::env::var("SystemRoot") {
        return std::path::PathBuf::from(systemroot)
            .join(r"System32\cmd.exe")
            .to_string_lossy()
            .into_owned();
    }
    "C:\\Windows\\System32\\cmd.exe".to_string()
}
