//! Local Inter-Process Communication (IPC) for daemon control.
//!
//! This module provides a local socket listener (Unix Domain Socket on Unix,
//! TCP loopback on Windows) that allows the CLI to send commands to a running
//! irosh background service.
//!
//! On Windows every command must carry a per-instance auth token. The listener
//! binds to `127.0.0.1`, which any local process can reach, so without the
//! token a random user could otherwise disable wormholes or shut the daemon
//! down. The token is written to the state directory (`ipc.token`) and read by
//! [`crate::client::ipc::IpcClient`]. Unix relies on socket permissions and
//! sends bare commands.

use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::collections::hash_map::DefaultHasher;
#[cfg(unix)]
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info, warn};

/// Conservative Unix domain socket path limit.
///
/// macOS allows 104 bytes for `sun_path`, Linux 108. Staying under 100
/// keeps clear of both when appending NULs or platform quirks.
#[cfg(unix)]
const MAX_SOCKET_PATH_LEN: usize = 100;

/// Returns the IPC control socket path for a given state directory.
///
/// The normal location is `<state_dir>/irosh.sock`, but deep state
/// directories (for example CI runners under `/var/folders/...` on macOS)
/// can push it past `sun_path`. In that case a short, deterministic path is
/// derived from a hash of the would-be path and placed in a private per-user
/// directory under the temp dir, so the daemon and the CLI stay in agreement
/// and the control socket is not exposed to other local users.
#[must_use]
pub(crate) fn socket_path(state_dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        let candidate = state_dir.join("irosh.sock");
        if candidate.as_os_str().len() <= MAX_SOCKET_PATH_LEN {
            return candidate;
        }
        let mut hasher = DefaultHasher::new();
        candidate.hash(&mut hasher);
        let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
        std::env::temp_dir()
            .join(format!("irosh-ipc-{user}"))
            .join(format!("irosh-{:016x}.sock", hasher.finish()))
    }
    #[cfg(windows)]
    {
        state_dir.join("ipc.port")
    }
}

/// Commands that can be sent to the irosh daemon via IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum IpcCommand {
    /// Enable a wormhole pairing session.
    EnableWormhole {
        /// The human-friendly 3-word code or custom string.
        code: String,
        /// Optional session password for additional security.
        password: Option<String>,
        /// Whether the wormhole should survive a reboot.
        persistent: bool,
    },
    /// Immediately disable any active wormhole.
    DisableWormhole,
    /// Query the current status of the daemon.
    GetStatus,
    /// Request a graceful shutdown of the daemon.
    Shutdown,
}

/// Envelope wrapping an IPC command with the instance auth token on Windows.
///
/// The Windows listener is a TCP loopback socket that is reachable by any
/// local process, so the daemon generates a random token per instance and
/// requires every command to carry it. The token file lives next to
/// `ipc.port` in the state directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg(windows)]
pub struct IpcEnvelope {
    /// Per-instance secret authorizing the wrapped command.
    pub token: String,
    /// The command to execute.
    pub command: IpcCommand,
}

/// Compares the presented IPC token against the expected token in constant
/// time.
///
/// The token is a shared secret between the daemon and its CLI, so a plain
/// `==` would let a local attacker distinguish a correct prefix (or otherwise
/// probe the token byte-by-byte) through response timing. `subtle`'s
/// `ConstantTimeEq` also covers length mismatches without short-circuiting.
#[cfg(windows)]
fn tokens_match(expected: &str, candidate: &str) -> bool {
    use subtle::ConstantTimeEq;
    expected.as_bytes().ct_eq(candidate.as_bytes()).into()
}

/// Internal version of IpcCommand that includes a response channel.
#[non_exhaustive]
pub enum InternalCommand {
    /// Enable the wormhole pairing mechanism with the given code and optional password.
    EnableWormhole {
        /// The human-friendly wormhole code.
        code: String,
        /// Optional password protecting the wormhole.
        password: Option<String>,
        /// Whether the wormhole should remain active across daemon restarts.
        persistent: bool,
        /// Channel to send the response back to the caller.
        tx: tokio::sync::oneshot::Sender<IpcResponse>,
    },
    /// Disable an active wormhole.
    DisableWormhole {
        /// Channel to send the response back to the caller.
        tx: tokio::sync::oneshot::Sender<IpcResponse>,
    },
    /// Query the current daemon status.
    GetStatus {
        /// Channel to send the response back to the caller.
        tx: tokio::sync::oneshot::Sender<IpcResponse>,
    },
    /// Request a graceful shutdown of the daemon.
    Shutdown {
        /// Channel to send the response back to the caller.
        tx: tokio::sync::oneshot::Sender<IpcResponse>,
    },
}

/// Detailed information about an active peer session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStatus {
    /// The remote peer's unique node ID.
    pub peer_id: String,
    /// When the session started (RFC3339).
    pub started_at: String,
    /// Total bytes sent to this peer.
    pub bytes_sent: u64,
    /// Total bytes received from this peer.
    pub bytes_received: u64,
}

/// Detailed daemon status information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonStatus {
    /// The server's unique P2P identifier.
    pub endpoint_id: String,
    /// The connection ticket for this node.
    pub ticket: String,
    /// Whether a wormhole is currently active.
    pub wormhole_active: bool,
    /// The active wormhole code (if any).
    pub wormhole_code: Option<String>,
    /// Number of active SSH sessions.
    pub active_sessions: usize,
    /// Rich information about each active session.
    pub sessions: Vec<SessionStatus>,
}

/// Responses sent by the irosh daemon back to the IPC client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum IpcResponse {
    /// Command was accepted and executed successfully.
    Ok,
    /// Command failed with a specific error message.
    Error(String),
    /// Current daemon status information.
    Status(DaemonStatus),
}

/// Errors specific to the IPC subsystem.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IpcError {
    /// Failed to bind the IPC socket.
    #[error("failed to bind ipc socket at {path}")]
    BindFailed {
        /// The socket path that could not be bound.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// An I/O error occurred during IPC communication.
    #[error("ipc i/o error")]
    Io(#[from] std::io::Error),

    /// Serialization or deserialization of IPC messages failed.
    #[error("ipc message serialization failed")]
    Serialization(#[from] serde_json::Error),
}

/// The IPC listener that handles incoming control commands.
pub struct IpcServer {
    state_dir: PathBuf,
    control_tx: tokio::sync::mpsc::Sender<InternalCommand>,
}

impl IpcServer {
    /// Creates a new IPC server using the provided state directory for the socket path.
    #[must_use]
    pub fn new(state_dir: PathBuf, control_tx: tokio::sync::mpsc::Sender<InternalCommand>) -> Self {
        Self {
            state_dir,
            control_tx,
        }
    }

    /// Returns the platform-specific socket path.
    fn socket_path(&self) -> PathBuf {
        socket_path(&self.state_dir)
    }

    /// Starts the IPC listener loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the IPC socket cannot be bound or if a critical I/O error occurs.
    #[must_use]
    pub async fn run(
        self,
        mut shutdown_rx: tokio::sync::mpsc::Receiver<()>,
    ) -> std::result::Result<(), IpcError> {
        let path = self.socket_path();

        #[cfg(unix)]
        {
            // Deep state directories fall back to a per-user directory under
            // the temp dir; make sure it exists and is private.
            if path.starts_with(&std::env::temp_dir()) {
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                            .await;
                }
            }

            // Remove existing socket file if it exists.
            if path.exists() {
                let _ = tokio::fs::remove_file(&path).await;
            }

            let listener =
                tokio::net::UnixListener::bind(&path).map_err(|e| IpcError::BindFailed {
                    path: path.clone(),
                    source: e,
                })?;

            info!("IPC listener active at {}", path.display());

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("IPC server received shutdown signal, exiting.");
                        break;
                    }
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((mut stream, _)) => {
                                let tx = self.control_tx.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handle_ipc_connection(&mut stream, tx, None).await
                                    {
                                        debug!("IPC connection error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                warn!("IPC accept error: {}", e);
                            }
                        }
                    }
                }
            }

            let _ = tokio::fs::remove_file(&path).await;
            Ok(())
        }

        #[cfg(windows)]
        {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| IpcError::BindFailed {
                    path: path.clone(),
                    source: e,
                })?;

            let local_addr = listener.local_addr().map_err(|e| IpcError::BindFailed {
                path: path.clone(),
                source: e,
            })?;
            let _ = tokio::fs::write(&path, local_addr.port().to_string()).await;

            // Write a per-instance auth token for the loopback listener.
            let token: [u8; 32] = rand::random();
            let token_hex = hex::encode(token);
            let _ = tokio::fs::write(path.with_file_name("ipc.token"), &token_hex).await;

            info!("IPC listener active on {}", local_addr);

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("IPC server received shutdown signal, exiting.");
                        break;
                    }
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((mut stream, _)) => {
                                let tx = self.control_tx.clone();
                                let expected_token = token_hex.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handle_ipc_connection(
                                        &mut stream,
                                        tx,
                                        Some(expected_token),
                                    )
                                    .await
                                    {
                                        debug!("IPC connection error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                warn!("IPC accept error: {}", e);
                            }
                        }
                    }
                }
            }

            let _ = tokio::fs::remove_file(&path).await;
            let _ = tokio::fs::remove_file(path.with_file_name("ipc.token")).await;
            Ok(())
        }
    }
}

/// Handles a single IPC connection.
///
/// On Windows `expected_token` is the per-instance auth token that every
/// command must carry. On Unix it is `None` and socket permissions provide
/// the access control.
async fn handle_ipc_connection<S>(
    stream: &mut S,
    control_tx: tokio::sync::mpsc::Sender<InternalCommand>,
    expected_token: Option<String>,
) -> std::result::Result<(), IpcError>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    // Use a reasonable limit for IPC messages to prevent DoS.
    let mut buf = Vec::with_capacity(4096);
    stream.take(1024 * 64).read_to_end(&mut buf).await?;

    #[cfg(windows)]
    let command: IpcCommand = {
        let envelope: IpcEnvelope = serde_json::from_slice(&buf)?;
        if !expected_token.is_some_and(|expected| tokens_match(&expected, &envelope.token)) {
            debug!("Rejecting IPC command with invalid auth token");
            let res_buf = serde_json::to_vec(&IpcResponse::Error(
                "unauthorized: invalid ipc token".to_string(),
            ))?;
            stream.write_all(&res_buf).await?;
            stream.flush().await?;
            return Ok(());
        }
        envelope.command
    };

    #[cfg(unix)]
    let command: IpcCommand = serde_json::from_slice(&buf)?;
    #[cfg(unix)]
    let _ = expected_token;

    debug!("Received IPC command: {:?}", command);

    let (res_tx, res_rx) = tokio::sync::oneshot::channel();

    let internal_cmd = match command {
        IpcCommand::EnableWormhole {
            code,
            password,
            persistent,
        } => InternalCommand::EnableWormhole {
            code,
            password,
            persistent,
            tx: res_tx,
        },
        IpcCommand::DisableWormhole => InternalCommand::DisableWormhole { tx: res_tx },
        IpcCommand::GetStatus => InternalCommand::GetStatus { tx: res_tx },
        IpcCommand::Shutdown => InternalCommand::Shutdown { tx: res_tx },
    };

    let response = if control_tx.send(internal_cmd).await.is_ok() {
        res_rx.await.unwrap_or(IpcResponse::Error(
            "Server failed to provide a response".to_string(),
        ))
    } else {
        IpcResponse::Error("Server control channel closed".to_string())
    };

    let res_buf = serde_json::to_vec(&response)?;
    stream.write_all(&res_buf).await?;
    stream.flush().await?;

    Ok(())
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::socket_path;
    use std::path::Path;

    const LONG_DIR: &str = "/var/folders/3c/9yk3dkb1qgm34xb7828lhhvhrlznsnz/T/irosh-test-server-rate-limit-1700000000123456789longer-than-the-limit";

    #[test]
    fn socket_path_uses_state_dir_for_short_paths() {
        let p = socket_path(Path::new("/tmp/irosh"));
        assert_eq!(p, Path::new("/tmp/irosh").join("irosh.sock").into());
    }

    #[test]
    fn socket_path_shrinks_deep_state_dirs() {
        let p = socket_path(Path::new(LONG_DIR));
        assert!(
            p.as_os_str().len() <= 100,
            "socket path too long: {}",
            p.display()
        );
        assert!(p.to_string_lossy().contains("irosh-ipc-"));
    }

    #[test]
    fn socket_path_is_deterministic_for_same_state_dir() {
        let a = socket_path(Path::new(LONG_DIR));
        let b = socket_path(Path::new(LONG_DIR));
        assert_eq!(a, b);
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::tokens_match;

    #[test]
    fn tokens_match_accepts_only_exact_token() {
        assert!(tokens_match(
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"
        ));
    }

    #[test]
    fn tokens_match_rejects_length_mismatch() {
        assert!(!tokens_match(
            "a1b2c3d4e5f6a7b8",
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"
        ));
    }

    #[test]
    fn tokens_match_rejects_any_substitution() {
        assert!(!tokens_match(
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d7"
        ));
    }
}
