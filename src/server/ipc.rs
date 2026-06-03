//! Local Inter-Process Communication (IPC) for daemon control.
//!
//! This module provides a local socket listener (Unix Domain Socket on Unix,
//! Named Pipe on Windows) that allows the CLI to send commands to a running
//! irosh background service.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info, warn};

/// Commands that can be sent to the irosh daemon via IPC.
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
        #[cfg(unix)]
        {
            self.state_dir.join("irosh.sock")
        }
        #[cfg(windows)]
        {
            self.state_dir.join("ipc.port")
        }
    }

    /// Starts the IPC listener loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the IPC socket cannot be bound or if a critical I/O error occurs.
    pub async fn run(
        self,
        mut shutdown_rx: tokio::sync::mpsc::Receiver<()>,
    ) -> std::result::Result<(), IpcError> {
        let path = self.socket_path();

        #[cfg(unix)]
        {
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
                                    if let Err(e) = handle_ipc_connection(&mut stream, tx).await {
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
                                tokio::spawn(async move {
                                    if let Err(e) = handle_ipc_connection(&mut stream, tx).await {
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
    }
}

/// Handles a single IPC connection.
async fn handle_ipc_connection<S>(
    stream: &mut S,
    control_tx: tokio::sync::mpsc::Sender<InternalCommand>,
) -> std::result::Result<(), IpcError>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    // Use a reasonable limit for IPC messages to prevent DoS.
    let mut buf = Vec::with_capacity(4096);
    stream.take(1024 * 64).read_to_end(&mut buf).await?;

    let command: IpcCommand = serde_json::from_slice(&buf)?;
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
