//! Active remote session registry and metrics.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize};

use tokio::sync::Mutex;

use crate::config::PeerId;

use super::ipc::SessionStatus;

/// A currently connected remote peer session.
#[derive(Debug, Clone)]
pub(crate) struct ActiveSession {
    /// The remote peer's node identifier.
    pub(crate) peer_id: PeerId,
    /// Timestamp when this session was established.
    pub(crate) started_at: chrono::DateTime<chrono::Utc>,
    /// Total bytes transmitted to the peer.
    pub(crate) bytes_sent: Arc<AtomicU64>,
    /// Total bytes received from the peer.
    pub(crate) bytes_received: Arc<AtomicU64>,
}

/// Manages the set of active remote sessions.
#[derive(Default, Clone)]
pub(crate) struct SessionTracker {
    /// Map of session IDs to active session state.
    pub(crate) sessions: Arc<Mutex<HashMap<usize, ActiveSession>>>,
    /// Monotonically increasing session ID counter.
    pub(crate) next_id: Arc<AtomicUsize>,
}

impl SessionTracker {
    /// Creates a new empty tracker.
    pub(crate) fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Registers a new session and returns its ID and byte-counting atomics.
    pub(crate) async fn register(
        &self,
        peer_id: PeerId,
    ) -> (usize, Arc<AtomicU64>, Arc<AtomicU64>) {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let sent = Arc::new(AtomicU64::new(0));
        let received = Arc::new(AtomicU64::new(0));
        let session = ActiveSession {
            peer_id,
            started_at: chrono::Utc::now(),
            bytes_sent: sent.clone(),
            bytes_received: received.clone(),
        };
        self.sessions.lock().await.insert(id, session);
        (id, sent, received)
    }

    /// Removes a session from the tracker by its ID.
    pub(crate) async fn unregister(&self, id: usize) {
        self.sessions.lock().await.remove(&id);
    }

    /// Returns a snapshot of all active sessions for IPC reporting.
    pub(crate) async fn snapshot(&self) -> Vec<SessionStatus> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| SessionStatus {
                peer_id: s.peer_id.clone(),
                started_at: s.started_at.to_rfc3339(),
                bytes_sent: s.bytes_sent.load(std::sync::atomic::Ordering::Relaxed),
                bytes_received: s.bytes_received.load(std::sync::atomic::Ordering::Relaxed),
            })
            .collect()
    }
}
