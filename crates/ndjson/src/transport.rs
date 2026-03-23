use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::types::{ClaudeEvent, UserInput};

/// Wraps Claude Code's stdio (or any async byte streams) as an NDJSON channel.
///
/// `next_event()` reads one NDJSON line and deserialises it.
/// `send()` serialises a `UserInput` and writes it as a newline-terminated JSON line.
pub struct NdjsonTransport {
    stdin: Box<dyn AsyncWrite + Send + Unpin>,
    lines: tokio::io::Lines<BufReader<Box<dyn tokio::io::AsyncRead + Send + Unpin>>>,
}

impl NdjsonTransport {
    /// Construct from boxed async IO streams.
    pub fn new(
        stdin: Box<dyn AsyncWrite + Send + Unpin>,
        stdout: Box<dyn tokio::io::AsyncRead + Send + Unpin>,
    ) -> Self {
        Self {
            stdin,
            lines: BufReader::new(stdout).lines(),
        }
    }

    /// Convenience constructor from a tokio child process.
    pub fn from_child(child: &mut tokio::process::Child) -> Result<Self> {
        let stdin = child.stdin.take().context("child stdin was not piped")?;
        let stdout = child.stdout.take().context("child stdout was not piped")?;
        Ok(Self::new(Box::new(stdin), Box::new(stdout)))
    }

    /// Construct from a bollard container attach response.
    ///
    /// Bollard gives us a demuxed output stream and a write half. We filter
    /// the output stream to only stdout messages and convert to a byte reader.
    #[cfg(feature = "bollard")]
    pub fn from_bollard_attach(attach: bollard::container::AttachContainerResults) -> Self {
        use bytes::Bytes;
        use futures_util::StreamExt;
        use tokio_util::io::StreamReader;

        // Filter the multiplexed output to only stdout messages, then convert
        // into a byte stream so StreamReader can build an AsyncRead from it.
        let stdout_stream = attach.output.filter_map(|result| {
            let item: Option<std::io::Result<Bytes>> = match result {
                Ok(bollard::container::LogOutput::StdOut { message }) => {
                    Some(Ok(message))
                }
                Ok(_) => None,
                Err(e) => Some(Err(std::io::Error::other(e))),
            };
            std::future::ready(item)
        });

        let reader = StreamReader::new(stdout_stream);
        let writer = BollardStdinWriter { inner: attach.input };
        Self::new(Box::new(writer), Box::new(reader))
    }

    /// Read the next NDJSON event. Returns `None` on EOF.
    pub async fn next_event(&mut self) -> Result<Option<ClaudeEvent>> {
        loop {
            let line = self.lines.next_line().await.context("reading NDJSON line")?;

            let Some(line) = line else {
                return Ok(None);
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            tracing::trace!(line = trimmed, "ndjson received");
            let event =
                serde_json::from_str::<ClaudeEvent>(trimmed).context("deserialising ClaudeEvent")?;
            return Ok(Some(event));
        }
    }

    /// Shut down the stdin writer, signalling EOF to the subprocess / container.
    pub async fn close_stdin(&mut self) {
        let _ = self.stdin.shutdown().await;
    }

    /// Serialise `input` as a JSON line and flush.
    pub async fn send(&mut self, input: &UserInput) -> Result<()> {
        let json = serde_json::to_string(input).context("serialising UserInput")?;
        tracing::trace!(json = json.as_str(), "ndjson sending");
        self.stdin
            .write_all(json.as_bytes())
            .await
            .context("writing UserInput")?;
        self.stdin.write_all(b"\n").await.context("writing newline")?;
        self.stdin.flush().await.context("flushing stdin")?;
        Ok(())
    }
}

/// Thin wrapper to make the bollard stdin usable as `AsyncWrite + Unpin`.
#[cfg(feature = "bollard")]
struct BollardStdinWriter {
    inner: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
}

#[cfg(feature = "bollard")]
impl AsyncWrite for BollardStdinWriter {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.inner.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_shutdown(cx)
    }
}
