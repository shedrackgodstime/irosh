//! Centralized timeout configuration for irosh.
//!
//! This module brings together all timeout values used across the codebase
//! into a single location for easy maintenance and documentation.

use std::time::Duration;

/// P2P connection timeout - how long to wait for a connection to establish
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Metadata session open timeout
pub const METADATA_OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// Metadata request timeout
pub const METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Discovery endpoint teardown timeout during wormhole rendezvous
pub const DISCOVERY_ENDPOINT_TEARDOWN_TIMEOUT: Duration = Duration::from_secs(300);

/// Session timeout for blob/dial operations
pub const SESSION_TIMEOUT: Duration = Duration::from_secs(3600);

/// Timeout for process execution control operations
pub const EXEC_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for waiting endpoint to come online
pub const ONLINE_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for client connection attempts
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);

/// Upper bound on how long the reader task waits for a PTY child to exit after
/// the PTY has reached EOF. A process that detached from the PTY (redirected
/// its stdio and keeps running) would otherwise block teardown indefinitely.
pub const CHILD_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Auth rejection delay - delay between auth rejection attempts
pub const AUTH_REJECTION_DELAY: Duration = Duration::from_secs(1);

/// Timeout for wormhole expiration.
///
/// After this duration, a wormhole is considered expired and cleaned up.
pub const WORMHOLE_EXPIRY_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

/// Authentication lockout window
pub const LOCKOUT_WINDOW: Duration = Duration::from_secs(60);

/// Autoreply window for DSR protocol
pub const DSR_AUTOREPLY_WINDOW: Duration = Duration::from_secs(10);

/// Connection retry timeout
pub const CONNECTION_RETRY_TIMEOUT: Duration = Duration::from_secs(5);

/// IPC request timeout
pub const IPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for individual IPC command operations.
///
/// Used when waiting for a response to an IPC command before considering
/// the command timed out.
pub const IPC_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
