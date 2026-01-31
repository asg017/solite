//! Helper traits and utilities for sending Jupyter protocol messages.
//!
//! This module provides the `JupyterSender` trait to reduce boilerplate
//! when sending messages through Jupyter channels.

use anyhow::Result;
use jupyter_protocol::{ClearOutput, DisplayData, JupyterMessage, MediaType, StreamContent};
use tokio::sync::mpsc;

/// Trait for sending Jupyter protocol messages with consistent error handling.
///
/// Implementations of this trait provide a unified interface for sending
/// various types of Jupyter messages (display data, streams, clear output, etc.)
/// without the repetitive `.await.unwrap()` pattern.
#[allow(async_fn_in_trait)]
pub trait JupyterSender {
    /// Send display data to the frontend.
    async fn send_display(&self, data: DisplayData, parent: &JupyterMessage) -> Result<()>;

    /// Send stream content (stdout/stderr) to the frontend.
    async fn send_stream(&self, content: StreamContent, parent: &JupyterMessage) -> Result<()>;

    /// Send a clear output message to the frontend.
    async fn send_clear(&self, wait: bool, parent: &JupyterMessage) -> Result<()>;

    /// Send plain text display data.
    async fn send_plain(&self, text: impl Into<String>, parent: &JupyterMessage) -> Result<()> {
        self.send_display(DisplayData::from(MediaType::Plain(text.into())), parent)
            .await
    }

    /// Send markdown display data.
    async fn send_markdown(&self, text: impl Into<String>, parent: &JupyterMessage) -> Result<()> {
        self.send_display(
            DisplayData::from(MediaType::Markdown(text.into())),
            parent,
        )
        .await
    }

    /// Send HTML display data.
    async fn send_html(&self, html: impl Into<String>, parent: &JupyterMessage) -> Result<()> {
        self.send_display(DisplayData::from(MediaType::Html(html.into())), parent)
            .await
    }

    /// Send stdout stream content.
    async fn send_stdout(&self, text: &str, parent: &JupyterMessage) -> Result<()> {
        self.send_stream(StreamContent::stdout(text), parent).await
    }

    /// Send stderr stream content.
    async fn send_stderr(&self, text: &str, parent: &JupyterMessage) -> Result<()> {
        self.send_stream(StreamContent::stderr(text), parent).await
    }
}

/// Implementation of `JupyterSender` for `mpsc::Sender<JupyterMessage>`.
///
/// This allows using the standard tokio mpsc channel as a JupyterSender,
/// which is useful for the async message handling in the kernel.
impl JupyterSender for mpsc::Sender<JupyterMessage> {
    async fn send_display(&self, data: DisplayData, parent: &JupyterMessage) -> Result<()> {
        self.send(data.as_child_of(parent))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send display data: {}", e))
    }

    async fn send_stream(&self, content: StreamContent, parent: &JupyterMessage) -> Result<()> {
        self.send(content.as_child_of(parent))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send stream content: {}", e))
    }

    async fn send_clear(&self, wait: bool, parent: &JupyterMessage) -> Result<()> {
        self.send(ClearOutput { wait }.as_child_of(parent))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send clear output: {}", e))
    }
}
