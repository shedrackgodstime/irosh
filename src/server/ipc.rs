//! Local Inter-Process Communication (IPC) for daemon control.
//!
//! This module provides a local socket listener (Unix Domain Socket on Unix,
//! Named Pipe on Windows) that allows the CLI to send commands to a running
//! irosh background service.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[allow(unused_imports)]
use tracing::{debug, info, warn};

/// Commands that can be sent to the irosh daemon via IPC.
#[derive(Debug, Serialize, Deserialize)]
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
pub enum InternalCommand {
    EnableWormhole {
        code: String,
        password: Option<String>,
        persistent: bool,
        tx: tokio::sync::oneshot::Sender<IpcResponse>,
    },
    DisableWormhole {
        tx: tokio::sync::oneshot::Sender<IpcResponse>,
    },
    GetStatus {
        tx: tokio::sync::oneshot::Sender<IpcResponse>,
    },
    Shutdown {
        tx: tokio::sync::oneshot::Sender<IpcResponse>,
    },
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
}

/// Responses sent by the irosh daemon back to the IPC client.
#[derive(Debug, Serialize, Deserialize)]
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
pub enum IpcError {
    /// Failed to bind the IPC socket.
    #[error("failed to bind ipc socket at {path}")]
    BindFailed {
        path: PathBuf,
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
    pub async fn run(
        self,
        mut shutdown_rx: tokio::sync::mpsc::Receiver<()>,
    ) -> std::result::Result<(), IpcError> {
        let path = self.socket_path();

        #[cfg(unix)]
        {
            // Remove existing socket file if it exists.
            if path.exists() {
                let _ = std::fs::remove_file(&path);
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

            let _ = std::fs::remove_file(&path);
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
            let _ = std::fs::write(&path, local_addr.port().to_string());

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

            let _ = std::fs::remove_file(&path);
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
    let mut buf = Vec::new();
    // Use a reasonable limit for IPC messages to prevent DoS.
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
