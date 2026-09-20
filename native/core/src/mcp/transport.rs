//! Shared bounded newline transport for client and server roles.
use super::{Diagnostics, FRAME_LIMIT, TOTAL_LIMIT};
use rmcp::{
    service::{RxJsonRpcMessage, ServiceRole, TxJsonRpcMessage},
    transport::Transport,
};
use std::{
    future::Future,
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
pub(super) struct BoundedStdio<R: ServiceRole> {
    _role: std::marker::PhantomData<R>,
    outgoing_limit: usize,
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: Arc<tokio::sync::Mutex<Option<Box<dyn AsyncWrite + Unpin + Send>>>>,
    line: Vec<u8>,
    total: usize,
    window: Instant,
    frames: usize,
    diagnostics: Diagnostics,
    cancel: CancellationToken,
}
impl<R: ServiceRole> BoundedStdio<R> {
    pub(super) fn new(
        reader: impl AsyncRead + Unpin + Send + 'static,
        writer: impl AsyncWrite + Unpin + Send + 'static,
        diagnostics: Diagnostics,
        cancel: CancellationToken,
        outgoing_limit: usize,
    ) -> Self {
        Self {
            _role: std::marker::PhantomData,
            outgoing_limit,
            reader: BufReader::new(Box::new(reader)),
            writer: Arc::new(tokio::sync::Mutex::new(Some(Box::new(writer)))),
            line: Vec::new(),
            total: 0,
            window: Instant::now(),
            frames: 0,
            diagnostics,
            cancel,
        }
    }

    fn stop(&self, message: &str) {
        self.diagnostics.fail(message);
        self.cancel.cancel();
    }
}
impl<R: ServiceRole> Transport<R> for BoundedStdio<R> {
    type Error = io::Error;
    fn send(
        &mut self,
        item: TxJsonRpcMessage<R>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let writer = self.writer.clone();
        let limit = self.outgoing_limit;
        let cancel = self.cancel.clone();
        async move {
            let mut bytes = serde_json::to_vec(&item)?;
            if bytes.len() > limit {
                return Err(io::Error::other("MCP outgoing frame exceeds limit"));
            }
            bytes.push(b'\n');
            let write = async {
                let mut guard = writer.lock().await;
                let writer = guard
                    .as_mut()
                    .ok_or_else(|| io::Error::other("MCP transport closed"))?;
                writer.write_all(&bytes).await?;
                writer.flush().await
            };
            tokio::select! {
                _ = cancel.cancelled() => Err(io::Error::other("MCP transport cancelled")),
                result = tokio::time::timeout(Duration::from_secs(5),write) => match result {
                    Ok(result) => result,
                    Err(_) => {cancel.cancel(); Err(io::Error::other("MCP peer is not reading responses"))}
                }
            }
        }
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<R>> {
        loop {
            // fill_buf/consume and the persistent line are cancellation-safe:
            // a simultaneous outgoing message cannot discard a partial reply.
            let buffer = match self.reader.fill_buf().await {
                Ok(buffer) => buffer,
                Err(_) => {
                    self.stop("MCP stdout read failed");
                    return None;
                }
            };
            if buffer.is_empty() {
                self.stop(if self.line.is_empty() {
                    "MCP server closed stdout"
                } else {
                    "MCP server ended with an incomplete frame"
                });
                return None;
            }
            let end = buffer.iter().position(|b| *b == b'\n').map(|i| i + 1);
            let count = end.unwrap_or(buffer.len());
            if self.line.len() + count > FRAME_LIMIT || self.total + count > TOTAL_LIMIT {
                self.stop("MCP incoming frame or connection byte limit exceeded");
                return None;
            }
            self.line.extend_from_slice(&buffer[..count]);
            self.total += count;
            self.reader.consume(count);
            if end.is_none() {
                continue;
            }
            if self.window.elapsed() >= Duration::from_secs(1) {
                self.window = Instant::now();
                self.frames = 0;
            }
            self.frames += 1;
            if self.frames > 128 {
                self.stop("MCP incoming frame rate exceeded");
                return None;
            }
            if self.line.iter().all(u8::is_ascii_whitespace) {
                self.line.clear();
                continue;
            }
            let parsed = serde_json::from_slice(&self.line);
            self.line.clear();
            return match parsed {
                Ok(message) => Some(message),
                Err(_) => {
                    self.stop("MCP server sent malformed JSON-RPC");
                    None
                }
            };
        }
    }
    async fn close(&mut self) -> io::Result<()> {
        self.cancel.cancel();
        self.writer.lock().await.take();
        Ok(())
    }
}
