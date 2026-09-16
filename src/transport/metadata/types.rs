//! Peer metadata types.
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Maximum length for any single metadata field.
const MAX_FIELD_LEN: usize = 255;

/// Connection metadata optionally exchanged on a separate control stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct PeerMetadata {
    /// The remote machine's hostname.
    pub hostname: String,
    /// The remote machine's user.
    pub user: String,
    /// The remote machine's operating system.
    pub os: String,
}

impl PeerMetadata {
    /// Creates new peer metadata, sanitizing all fields.
    #[must_use]
    pub fn new(hostname: String, user: String, os: String) -> Self {
        Self {
            hostname: Self::sanitize_field(hostname),
            user: Self::sanitize_field(user),
            os: Self::sanitize_field(os),
        }
    }

    /// Strips control characters (except space and tab) and truncates.
    fn sanitize_field(mut s: String) -> String {
        s.retain(|c| c >= ' ' || c == '\t');
        s.truncate(MAX_FIELD_LEN);
        s
    }

    /// Generates a friendly default alias like "kristency-linux".
    #[must_use]
    pub fn default_alias(&self) -> String {
        let clean_user = self.user.replace(' ', "-").to_lowercase();
        let clean_os = self.os.replace(' ', "-").to_lowercase();
        format!("{clean_user}-{clean_os}")
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
    #[must_use]
    pub async fn current() -> Self {
        tokio::task::spawn_blocking(move || {
            PeerMetadata::new(
                Self::resolve_hostname(),
                Self::resolve_username(),
                std::env::consts::OS.to_string(),
            )
        })
        .await
        .unwrap_or_else(|e| {
            warn!("failed to resolve local metadata: {e}");
            PeerMetadata::new(
                "unknown-host".to_string(),
                "unknown-user".to_string(),
                std::env::consts::OS.to_string(),
            )
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
        #[cfg(windows)]
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
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast::<libc::c_char>(), buf.len()) };
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
    // On Unix, getpwuid_r is most reliable - reads /etc/passwd even in daemon
    // context. The reentrant `_r` variant fills caller-owned buffers, so NSS
    // lookups triggered by other threads cannot clobber our entry mid-read.
    #[cfg(unix)]
    {
        // SAFETY: `getuid` takes no arguments and always succeeds.
        let uid = unsafe { libc::getuid() };
        // SAFETY: `zeroed()` is used only to provide a writable `passwd`
        // storage slot for `getpwuid_r`; this C struct contains no fields
        // whose all-zeros value is invalid prior to being overwritten by the
        // successful fill below.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut buf_len = 1024usize;
        loop {
            let mut buf = vec![0u8; buf_len];
            let mut result: *mut libc::passwd = std::ptr::null_mut();
            // SAFETY: `pwd`, `buf`, and `result` are caller-owned and live for
            // the duration of the call; `buf` is at least `buf.len()` bytes
            // and sized for a passwd entry at this length.
            let rc = unsafe {
                libc::getpwuid_r(
                    uid,
                    &mut pwd,
                    buf.as_mut_ptr().cast::<libc::c_char>(),
                    buf.len(),
                    &mut result,
                )
            };
            if rc == 0 {
                if !result.is_null() {
                    // SAFETY: on success `result` points at the filled `pwd`
                    // whose `pw_name` is a NUL-terminated C string.
                    let name = unsafe { std::ffi::CStr::from_ptr((*result).pw_name) };
                    if let Ok(s) = name.to_str()
                        && !s.is_empty()
                    {
                        return Some(s.to_string());
                    }
                }
                break;
            }
            if rc == libc::ERANGE && buf_len < 16 * 1024 {
                buf_len *= 2;
                continue;
            }
            break;
        }
    }

    // Universal subprocess fallback - `whoami` works on Linux, Windows, macOS
    let output = std::process::Command::new("whoami").output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // On Windows, whoami returns "DOMAIN\user" - strip the domain prefix
        // and reject the service-account aliases it reports in elevated
        // contexts. On other platforms whoami is already a plain username.
        #[cfg(windows)]
        {
            let s = s.rsplit('\\').next().unwrap_or(&s).to_string();
            if !s.is_empty()
                && !s.eq_ignore_ascii_case("system")
                && !s.eq_ignore_ascii_case("nt authority")
            {
                return Some(s);
            }
        }
        #[cfg(not(windows))]
        {
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

/// Error type for metadata framing and I/O.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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
