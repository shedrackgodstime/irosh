//! Runtime metrics counters for production observability.
//!
//! All counters use relaxed atomic ordering — they are intended for
//! operators, not for synchronisation.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Snapshot of all counter values at a point in time.
#[derive(Debug, Clone, Copy, Default)]
pub struct MetricsSnapshot {
    /// Total connections accepted since server start.
    pub total_connections: u64,
    /// Connections currently active.
    pub active_connections: u64,
    /// Total bytes sent by the server.
    pub bytes_sent: u64,
    /// Total bytes received by the server.
    pub bytes_received: u64,
    /// Total transfer operations initiated.
    pub transfers_initiated: u64,
    /// Transfer operations that completed successfully.
    pub transfers_completed: u64,
    /// Transfer operations that terminated with an error.
    pub transfers_failed: u64,
    /// Non-transfer protocol errors.
    pub errors_total: u64,
}

/// Runtime metrics counters for the full application.
///
/// Clone is cheap (clones the `Arc`).
#[derive(Clone, Default)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

#[derive(Default)]
struct MetricsInner {
    total_connections: AtomicU64,
    active_connections: AtomicU64,
    bytes_sent: AtomicU64,
    bytes_received: AtomicU64,
    transfers_initiated: AtomicU64,
    transfers_completed: AtomicU64,
    transfers_failed: AtomicU64,
    errors_total: AtomicU64,
}

/// A handle that decrements `active_connections` on drop.
///
/// Obtained from [`Metrics::register_connection`] and MUST be held alive
/// for the duration of the connection.
pub struct ConnectionGuard {
    metrics: Metrics,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.metrics
            .inner
            .active_connections
            .fetch_sub(1, Ordering::Relaxed);
    }
}

impl Metrics {
    /// Creates a new, zeroed metrics collector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new connection and returns a guard that decrements on drop.
    #[must_use]
    pub fn register_connection(&self) -> ConnectionGuard {
        self.inner.total_connections.fetch_add(1, Ordering::Relaxed);
        self.inner
            .active_connections
            .fetch_add(1, Ordering::Relaxed);
        ConnectionGuard {
            metrics: self.clone(),
        }
    }

    /// Records bytes sent over a connection.
    pub fn record_bytes_sent(&self, n: u64) {
        self.inner.bytes_sent.fetch_add(n, Ordering::Relaxed);
    }

    /// Records bytes received over a connection.
    pub fn record_bytes_received(&self, n: u64) {
        self.inner.bytes_received.fetch_add(n, Ordering::Relaxed);
    }

    /// Records a transfer initiation.
    pub fn record_transfer_initiated(&self) {
        self.inner
            .transfers_initiated
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Records a completed transfer.
    pub fn record_transfer_completed(&self) {
        self.inner
            .transfers_completed
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Records a failed transfer.
    pub fn record_transfer_failed(&self) {
        self.inner.transfers_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a non-transfer error.
    pub fn record_error(&self) {
        self.inner.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Takes an atomic snapshot of all counters.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            total_connections: self.inner.total_connections.load(Ordering::Relaxed),
            active_connections: self.inner.active_connections.load(Ordering::Relaxed),
            bytes_sent: self.inner.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.inner.bytes_received.load(Ordering::Relaxed),
            transfers_initiated: self.inner.transfers_initiated.load(Ordering::Relaxed),
            transfers_completed: self.inner.transfers_completed.load(Ordering::Relaxed),
            transfers_failed: self.inner.transfers_failed.load(Ordering::Relaxed),
            errors_total: self.inner.errors_total.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_start_at_zero() {
        let m = Metrics::new();
        let snap = m.snapshot();
        assert_eq!(snap.total_connections, 0);
        assert_eq!(snap.errors_total, 0);
    }

    #[test]
    fn connection_count_tracks_active() {
        let m = Metrics::new();
        {
            let _guard = m.register_connection();
            assert_eq!(m.snapshot().active_connections, 1);
        }
        assert_eq!(m.snapshot().active_connections, 0);
        assert_eq!(m.snapshot().total_connections, 1);
    }

    #[test]
    fn bytes_accumulate() {
        let m = Metrics::new();
        m.record_bytes_sent(100);
        m.record_bytes_sent(200);
        assert_eq!(m.snapshot().bytes_sent, 300);
    }

    #[test]
    fn metrics_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Metrics>();
    }
}
