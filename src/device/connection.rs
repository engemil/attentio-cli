use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, trace};

use crate::error::AttentioError;

/// Default baud rate for CDC serial ports.
/// CDC/ACM is virtual serial — baud rate is ignored by USB, but required by the API.
const DEFAULT_BAUD_RATE: u32 = 115200;

/// Default timeout for command responses.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// An async connection to a device over a serial port.
pub struct DeviceConnection {
    reader: BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>,
    writer: tokio::io::WriteHalf<tokio_serial::SerialStream>,
    timeout: Duration,
}

impl DeviceConnection {
    /// Open an async serial connection to the given port path.
    pub fn open(port_path: &str) -> Result<Self, AttentioError> {
        debug!("Opening serial port: {}", port_path);

        let port = tokio_serial::new(port_path, DEFAULT_BAUD_RATE)
            .open_native_async()
            .map_err(|e| AttentioError::Other(format!("Failed to open {}: {}", port_path, e)))?;

        let (read_half, write_half) = tokio::io::split(port);

        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Set the command response timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Send a command and wait for the response.
    ///
    /// Protocol: send `<cmd>\r\n`, read lines until `OK\r\n` or `ERROR <msg>\r\n`.
    /// Returns the response body (lines before the OK/ERROR terminator).
    pub async fn send_command(&mut self, cmd: &str) -> Result<String, AttentioError> {
        debug!("Sending command: {:?}", cmd);

        // Send command with \r\n terminator
        let cmd_bytes = format!("{}\r\n", cmd);
        self.writer
            .write_all(cmd_bytes.as_bytes())
            .await
            .map_err(AttentioError::Io)?;
        self.writer.flush().await.map_err(AttentioError::Io)?;

        trace!("Command sent, waiting for response...");

        // Read response lines until OK or ERROR
        let mut response_lines: Vec<String> = Vec::new();

        loop {
            let mut line = String::new();

            let read_result = tokio::time::timeout(self.timeout, async {
                self.reader
                    .read_line(&mut line)
                    .await
                    .map_err(AttentioError::Io)
            })
            .await;

            match read_result {
                Ok(Ok(0)) => {
                    return Err(AttentioError::Protocol {
                        message: "connection closed unexpectedly".to_string(),
                    });
                }
                Ok(Ok(_)) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    trace!("Received line: {:?}", trimmed);

                    if trimmed == "OK" {
                        debug!("Command completed successfully");
                        return Ok(response_lines.join("\n"));
                    } else if let Some(err_msg) = trimmed.strip_prefix("ERROR ") {
                        return Err(AttentioError::Protocol {
                            message: err_msg.to_string(),
                        });
                    } else if trimmed == "ERROR" {
                        return Err(AttentioError::Protocol {
                            message: "unknown error".to_string(),
                        });
                    } else {
                        response_lines.push(trimmed.to_string());
                    }
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(AttentioError::Timeout {
                        seconds: self.timeout.as_secs(),
                    });
                }
            }
        }
    }

    /// Read a single line from the port (useful for debug print streams).
    pub async fn read_line(&mut self) -> Result<String, AttentioError> {
        let mut line = String::new();

        let bytes_read = tokio::time::timeout(self.timeout, async {
            self.reader
                .read_line(&mut line)
                .await
                .map_err(AttentioError::Io)
        })
        .await;

        match bytes_read {
            Ok(Ok(0)) => Err(AttentioError::Protocol {
                message: "connection closed".to_string(),
            }),
            Ok(Ok(_)) => {
                let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                Ok(trimmed)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AttentioError::Timeout {
                seconds: self.timeout.as_secs(),
            }),
        }
    }

    /// Write raw bytes to the port (useful for interactive shell).
    #[allow(dead_code)] // Reserved for future shell/monitor improvements
    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), AttentioError> {
        self.writer
            .write_all(data)
            .await
            .map_err(AttentioError::Io)?;
        self.writer.flush().await.map_err(AttentioError::Io)?;
        Ok(())
    }
}
