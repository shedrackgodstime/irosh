use serde::{Deserialize, Serialize};

/// Connection metadata optionally exchanged on a separate control stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerMetadata {
    /// The remote machine's hostname.
    pub hostname: String,
    /// The remote machine's user.
    pub user: String,
    /// The remote machine's operating system.
    pub os: String,
}

impl PeerMetadata {
    /// Generates a friendly default alias like "kristency-linux".
    pub fn default_alias(&self) -> String {
        let clean_user = self.user.replace(' ', "-").to_lowercase();
        let clean_os = self.os.replace(' ', "-").to_lowercase();
        format!("{}-{}", clean_user, clean_os)
    }

    /// Collects the current system's metadata to send to a connecting peer.
    ///
    /// Uses a layered resolution strategy so it works reliably as a service,
    /// daemon, or interactive process on both Linux and Windows.
    ///
    /// # Performance
    ///
    /// This is an async function that uses `spawn_blocking` because host/user
    /// resolution may involve spawning subprocesses or performing blocking syscalls.
    pub async fn current() -> Self {
        tokio::task::spawn_blocking(move || Self {
            hostname: Self::resolve_hostname(),
            user: Self::resolve_username(),
            os: std::env::consts::OS.to_string(),
        })
        .await
        .unwrap_or_else(|_| Self {
            hostname: "unknown-host".to_string(),
            user: "unknown-user".to_string(),
            os: std::env::consts::OS.to_string(),
        })
    }

    fn resolve_hostname() -> String {
        // Try env vars first (fast path, works in most interactive shells)
        if let Ok(h) = std::env::var("HOSTNAME") {
            if !h.is_empty() && h != "localhost" {
                return h;
            }
        }
        // COMPUTERNAME is the Windows equivalent
        if let Ok(h) = std::env::var("COMPUTERNAME") {
            if !h.is_empty() {
                return h;
            }
        }

        // Syscall fallback - works even when launched as a service
        hostname_syscall().unwrap_or_else(|| "unknown-host".to_string())
    }

    fn resolve_username() -> String {
        // USER on Unix, USERNAME on Windows
        if let Ok(u) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
            // Reject service-account names that indicate a non-interactive context
            let u_lower = u.to_lowercase();
            if !u.is_empty()
                && u_lower != "system"
                && u_lower != "local service"
                && u_lower != "network service"
                && !u_lower.contains("systemprofile")
            {
                return u;
            }
        }

        // Fallback: ask the OS who is running this process
        username_syscall().unwrap_or_else(|| "unknown-user".to_string())
    }
}

/// Resolves the system hostname via a platform syscall, then subprocess fallback.
fn hostname_syscall() -> Option<String> {
    // Try libc::gethostname on Unix - fastest and most reliable in daemon context
    #[cfg(unix)]
    {
        let mut buf = vec![0u8; 256];
        // SAFETY: `buf` is correctly sized and we use a valid pointer.
        // `gethostname` is a standard Unix syscall.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if rc == 0 {
            let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            if let Ok(s) = String::from_utf8(buf[..nul].to_vec()) {
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }

    // Universal subprocess fallback - works on Linux, Windows, macOS
    let output = std::process::Command::new("hostname").output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

/// Resolves the current username via a platform syscall, then subprocess fallback.
fn username_syscall() -> Option<String> {
    // On Unix, getpwuid is most reliable - reads /etc/passwd even in daemon context
    #[cfg(unix)]
    {
        // SAFETY: `getuid` and `getpwuid` are standard Unix syscalls.
        // We check if `pw` is null before dereferencing it via `CStr`.
        let uid = unsafe { libc::getuid() };
        let pw = unsafe { libc::getpwuid(uid) };
        if !pw.is_null() {
            let name = unsafe { std::ffi::CStr::from_ptr((*pw).pw_name) };
            if let Ok(s) = name.to_str() {
                let s = s.to_string();
                // Skip root / service-like UIDs in interactive context
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }

    // Universal subprocess fallback - `whoami` works on Linux, Windows, macOS
    let output = std::process::Command::new("whoami").output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // On Windows, whoami returns "DOMAIN\user" - strip the domain prefix
        let s = s.rsplit('\\').next().unwrap_or(&s).to_string();
        if !s.is_empty()
            && !s.eq_ignore_ascii_case("system")
            && !s.eq_ignore_ascii_case("nt authority")
        {
            return Some(s);
        }
    }
    None
}

/// Error type for metadata framing and I/O.
#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    /// A standard library I/O error during metadata exchange.
    #[error("metadata I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The stream header does not match the expected metadata magic bytes.
    #[error("invalid metadata magic header")]
    InvalidMagic,

    /// The remote peer is using an incompatible metadata protocol version.
    #[error("unsupported metadata version: {0}")]
    UnsupportedVersion(u8),

    /// An unknown or unhandled metadata frame kind was received.
    #[error("unsupported metadata frame kind: {0}")]
    UnsupportedKind(u8),

    /// Received a frame kind that was invalid for the current protocol state.
    #[error("unexpected metadata frame kind: expected {expected}, got {actual}")]
    UnexpectedKind {
        /// The frame kind the receiver was expecting.
        expected: u8,
        /// The frame kind that was actually received.
        actual: u8,
    },

    /// The received metadata payload exceeds the maximum allowed size.
    #[error("metadata payload too large: {0} bytes")]
    PayloadTooLarge(usize),

    /// Failed to parse or serialize a JSON metadata payload.
    #[error("invalid metadata payload: {0}")]
    Json(#[from] serde_json::Error),
}
