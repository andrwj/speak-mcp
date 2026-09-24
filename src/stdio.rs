use anyhow::Result;
use async_mcp::transport::{Message, Transport};
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

// Yield while waiting for input so background playback can keep progressing.
pub struct StdioTransport {
    input: Mutex<BufReader<tokio::io::Stdin>>,
    output: Mutex<tokio::io::Stdout>,
}

impl StdioTransport {
    pub fn new() -> Self {
        Self {
            input: Mutex::new(BufReader::new(tokio::io::stdin())),
            output: Mutex::new(tokio::io::stdout()),
        }
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn receive(&self) -> Result<Option<Message>> {
        let mut line = String::new();
        if self.input.lock().await.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&line)?))
    }

    async fn send(&self, message: &Message) -> Result<()> {
        let mut data = serde_json::to_vec(message)?;
        data.push(b'\n');
        let mut output = self.output.lock().await;
        output.write_all(&data).await?;
        output.flush().await?;
        Ok(())
    }

    async fn open(&self) -> Result<()> {
        Ok(())
    }
    async fn close(&self) -> Result<()> {
        Ok(())
    }
}
