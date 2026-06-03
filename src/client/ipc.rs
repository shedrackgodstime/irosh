//! Client-side Inter-Process Communication (IPC) for daemon control.
//!
//! This module provides a client that can send commands to a running
//! irosh background service via a local socket.

use crate::error::Result;
use crate::server::ipc::{IpcCommand, IpcResponse};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A client for communicating with a running irosh daemon.
pub struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    /// Creates a new IPC client targeting the daemon in the specified state directory.
    #[must_use] 
    pub fn new(state_dir: &std::path::Path) -> Self {
        #[cfg(unix)]
        let socket_path = state_dir.join("irosh.sock");
        #[cfg(windows)]
        let socket_path = state_dir.join("ipc.port");

        Self { socket_path }
    }

    /// Sends a command to the daemon and waits for a response.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection to the daemon fails or the response cannot be deserialized.
    #[must_use]
    pub async fn send(&self, command: IpcCommand) -> Result<IpcResponse> {
        let mut stream = self.connect().await?;

        let buf = serde_json::to_vec(&command).map_err(|e| {
            crate::error::IroshError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;

        stream.write_all(&buf).await?;
        // Shutdown writing so the server knows the command is complete.
        stream.shutdown().await?;

        let mut res_buf = Vec::with_capacity(4096);
        stream.read_to_end(&mut res_buf).await?;

        let response: IpcResponse = serde_json::from_slice(&res_buf).map_err(|e| {
            crate::error::IroshError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;

        Ok(response)
    }

    #[cfg(unix)]
    async fn connect(&self) -> Result<tokio::net::UnixStream> {
        tokio::net::UnixStream::connect(&self.socket_path)
            .await
            .map_err(crate::error::IroshError::Io)
    }

    #[cfg(windows)]
    async fn connect(&self) -> Result<tokio::net::TcpStream> {
        let port_str =
            tokio::fs::read_to_string(&self.socket_path).await.map_err(crate::error::IroshError::Io)?;
        let port: u16 = port_str.trim().parse().map_err(|_| {
            crate::error::IroshError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid port in ipc.port file",
            ))
        })?;

        let addr = format!("127.0.0.1:{}", port);
        tokio::net::TcpStream::connect(&addr)
            .await
            .map_err(crate::error::IroshError::Io)
    }
}
