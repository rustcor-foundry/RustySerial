use anyhow::Result;
use chrono::Local;
use std::path::Path;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

/// Append-only logger. Each chunk of bytes from the serial port gets a
/// `[YYYY-MM-DD HH:MM:SS.mmm] ` prefix on a new line if the previous
/// chunk ended with `\n`, otherwise it's appended inline.
pub struct Logger {
    file: File,
    needs_timestamp: bool,
}

impl Logger {
    pub async fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        Ok(Self {
            file,
            needs_timestamp: true,
        })
    }

    pub async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if self.needs_timestamp {
            let stamp = Local::now().format("[%Y-%m-%d %H:%M:%S%.3f] ");
            self.file.write_all(stamp.to_string().as_bytes()).await?;
        }
        self.file.write_all(bytes).await?;
        self.needs_timestamp = bytes.last() == Some(&b'\n');
        // Flush each chunk so that if the user kills rustyserial, no data is lost.
        self.file.flush().await?;
        Ok(())
    }
}
