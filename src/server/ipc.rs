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
}

/// Responses sent by the irosh daemon back to the IPC client.
#[derive(Debug, Serialize, Deserialize)]
pub enum IpcResponse {
    /// Command was accepted and executed successfully.
    Ok,
    /// Command failed with a specific error message.
    Error(String),
    /// Current daemon status information.
    Status {
        /// Whether a wormhole is currently active.
        wormhole_active: bool,
        /// The active wormhole code (if any).
        wormhole_code: Option<String>,
        /// Number of active SSH sessions.
        active_sessions: usize,
    },
}

/// Errors specific to the IPC subsystem.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Failed to bind the IPC socket.
    #[error("failed to bind IPC socket at {path}")]
    BindFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// An I/O error occurred during IPC communication.
    #[error("IPC I/O error")]
    Io(#[from] std::io::Error),

    /// Serialization or deserialization of IPC messages failed.
    #[error("IPC message serialization failed")]
    Serialization(#[from] serde_json::Error),
}

/// The IPC listener that handles incoming control commands.
pub struct IpcServer {
    state_dir: PathBuf,
    control_tx: tokio::sync::mpsc::Sender<IpcCommand>,
}

impl IpcServer {
    /// Creates a new IPC server using the provided state directory for the socket path.
    pub fn new(state_dir: PathBuf, control_tx: tokio::sync::mpsc::Sender<IpcCommand>) -> Self {
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
            // Windows named pipes follow a specific format.
            // We use a name derived from the state directory hash or a fixed name.
            PathBuf::from(r"\\.\pipe\irosh-service")
        }
    }

    /// Starts the IPC listener loop.
    pub async fn run(self) -> std::result::Result<(), IpcError> {
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
                match listener.accept().await {
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

        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;

            info!("IPC listener active at {}", path.display());

            loop {
                let mut server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&path.to_string_lossy())?;

                server.connect().await?;

                let tx = self.control_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_ipc_connection(&mut server, tx).await {
                        debug!("IPC connection error: {}", e);
                    }
                });
            }
        }
    }
}

/// Handles a single IPC connection.
async fn handle_ipc_connection<S>(
    stream: &mut S,
    control_tx: tokio::sync::mpsc::Sender<IpcCommand>,
) -> std::result::Result<(), IpcError>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let mut buf = Vec::new();
    // Use a reasonable limit for IPC messages to prevent DoS.
    stream.take(1024 * 64).read_to_end(&mut buf).await?;

    let command: IpcCommand = serde_json::from_slice(&buf)?;
    debug!("Received IPC command: {:?}", command);

    // Send the command to the main server loop and wait for it to be processed
    // if needed (currently we just fire and forget for simple commands,
    // but we might want a response channel later).
    let response = match control_tx.send(command).await {
        Ok(_) => IpcResponse::Ok,
        Err(_) => IpcResponse::Error("Server control channel closed".to_string()),
    };

    let res_buf = serde_json::to_vec(&response)?;
    stream.write_all(&res_buf).await?;
    stream.flush().await?;

    Ok(())
}
