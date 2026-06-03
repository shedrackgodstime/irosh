//! Async read/write adapters for Iroh streams.

use std::pin::Pin;
use std::task::{Context, Poll};

use iroh::endpoint::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// An adapter wrapping an Iroh send and receive stream pair into a single
/// type implementing [`AsyncRead`] and [`AsyncWrite`].
pub struct IrohDuplex {
    send: SendStream,
    recv: Pin<Box<dyn AsyncRead + Send + Sync>>,
    bytes_sent: Option<Arc<AtomicU64>>,
    bytes_received: Option<Arc<AtomicU64>>,
}

impl std::fmt::Debug for IrohDuplex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohDuplex")
            .field("send", &"SendStream")
            .field("recv", &"Box<dyn AsyncRead + Send + Sync>")
            .field("bytes_sent", &self.bytes_sent)
            .field("bytes_received", &self.bytes_received)
            .finish()
    }
}

impl IrohDuplex {
    /// Creates a new `IrohDuplex` from an Iroh send/recv stream pair.
    #[must_use]
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Self {
            send,
            recv: Box::pin(recv),
            bytes_sent: None,
            bytes_received: None,
        }
    }

    /// Creates a new `IrohDuplex` that tracks transferred bytes.
    pub fn with_stats(
        send: SendStream,
        recv: RecvStream,
        bytes_tx: Arc<AtomicU64>,
        received: Arc<AtomicU64>,
    ) -> Self {
        Self {
            send,
            recv: Box::pin(recv),
            bytes_sent: Some(bytes_tx),
            bytes_received: Some(received),
        }
    }

    /// Creates a new `IrohDuplex` with a pre-read prefix buffer.
    /// This is useful for stream dispatching based on magic headers.
    #[must_use]
    pub fn with_prefix(send: SendStream, recv: RecvStream, prefix: Vec<u8>) -> Self {
        let chained = tokio::io::AsyncReadExt::chain(std::io::Cursor::new(prefix), recv);
        Self {
            send,
            recv: Box::pin(chained),
            bytes_sent: None,
            bytes_received: None,
        }
    }
}

impl AsyncRead for IrohDuplex {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.recv).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let read = buf.filled().len() - before;
                if let Some(counter) = &self.bytes_received {
                    counter.fetch_add(read as u64, Ordering::Relaxed);
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl AsyncWrite for IrohDuplex {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.send).poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => {
                if let Some(counter) = &self.bytes_sent {
                    counter.fetch_add(written as u64, Ordering::Relaxed);
                }
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(std::io::Error::other(err))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match Pin::new(&mut self.send).poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(std::io::Error::other(err))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match Pin::new(&mut self.send).poll_shutdown(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(std::io::Error::other(err))),
            Poll::Pending => Poll::Pending,
        }
    }
}

// Ensure at compile time that the struct holds the Send trait bound
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_irohduplex_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<IrohDuplex>();
    }
}
