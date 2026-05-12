//! Async read/write adapters for Iroh streams.

use std::pin::Pin;
use std::task::{Context, Poll};

use iroh::endpoint::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// An adapter wrapping an Iroh send and receive stream pair into a single
/// type implementing [`AsyncRead`] and [`AsyncWrite`].
pub struct IrohDuplex {
    send: SendStream,
    recv: Pin<Box<dyn AsyncRead + Send + Sync>>,
}

impl IrohDuplex {
    /// Creates a new `IrohDuplex` from an Iroh send/recv stream pair.
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Self {
            send,
            recv: Box::pin(recv),
        }
    }

    /// Creates a new `IrohDuplex` with a pre-read prefix buffer.
    /// This is useful for stream dispatching based on magic headers.
    pub fn with_prefix(send: SendStream, recv: RecvStream, prefix: Vec<u8>) -> Self {
        let chained = tokio::io::AsyncReadExt::chain(std::io::Cursor::new(prefix), recv);
        Self {
            send,
            recv: Box::pin(chained),
        }
    }
}

impl AsyncRead for IrohDuplex {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for IrohDuplex {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.send).poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => Poll::Ready(Ok(written)),
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
