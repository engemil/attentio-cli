//! High-level AP protocol client.
//!
//! Wraps a [`DeviceConnection`] and provides typed methods for each AP command.
//! The protocol is synchronous: send one command, receive one response.

use std::time::Duration;

use tracing::debug;

use crate::device::connection::DeviceConnection;
use crate::error::AttentioError;

use super::packet::{
    ap_error_name, build_packet, ApParser, ApResponse, CMD_GET_METADATA, CMD_SETTINGS_GET,
    CMD_SETTINGS_LIST, CMD_SETTINGS_SET,
};

/// Default timeout for AP command responses.
const AP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

/// AP protocol client wrapping a serial connection.
pub struct ApClient {
    conn: DeviceConnection,
    timeout: Duration,
}

impl ApClient {
    /// Create a new AP client from an open connection.
    pub fn new(conn: DeviceConnection) -> Self {
        Self {
            conn,
            timeout: AP_RESPONSE_TIMEOUT,
        }
    }

    /// Create a new AP client, opening a connection to the given port.
    pub fn open(port_path: &str) -> Result<Self, AttentioError> {
        let conn = DeviceConnection::open(port_path)?;
        Ok(Self::new(conn))
    }

    /// Set the response timeout.
    #[allow(dead_code)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Send an AP command and wait for the response.
    ///
    /// Builds the packet, writes it, then reads bytes from the port and feeds
    /// them into the AP parser state machine until a complete response is
    /// received or the timeout expires.
    pub async fn send_command(
        &mut self,
        cmd: u8,
        payload: &[u8],
    ) -> Result<ApResponse, AttentioError> {
        let pkt = build_packet(cmd, payload);
        debug!(
            "AP send: cmd=0x{:02X} payload_len={} pkt_len={}",
            cmd,
            payload.len(),
            pkt.len()
        );

        self.conn.write_raw(&pkt).await?;

        // Read response bytes with timeout
        let mut parser = ApParser::new();
        let mut buf = [0u8; 256];
        let deadline = tokio::time::Instant::now() + self.timeout;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(AttentioError::Timeout {
                    seconds: self.timeout.as_secs(),
                });
            }

            let n = match tokio::time::timeout(remaining, self.conn.read_raw(&mut buf)).await {
                Ok(Ok(0)) => {
                    return Err(AttentioError::Protocol {
                        message: "connection closed while waiting for AP response".to_string(),
                    });
                }
                Ok(Ok(n)) => n,
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(AttentioError::Timeout {
                        seconds: self.timeout.as_secs(),
                    });
                }
            };

            for &byte in &buf[..n] {
                if let Some(resp) = parser.feed(byte) {
                    debug!(
                        "AP recv: cmd=0x{:02X} payload_len={}",
                        resp.cmd,
                        resp.payload.len()
                    );
                    return Ok(resp);
                }
            }
        }
    }

    /// Send a command and return the payload on success, or an error with a
    /// human-readable message on failure.
    async fn send_command_ok(&mut self, cmd: u8, payload: &[u8]) -> Result<Vec<u8>, AttentioError> {
        let resp = self.send_command(cmd, payload).await?;

        if resp.is_ok() {
            Ok(resp.payload)
        } else if resp.is_error() {
            let code = resp.error_code().unwrap_or(0xFF);
            Err(AttentioError::Protocol {
                message: format!(
                    "device returned error 0x{:02X}: {}",
                    code,
                    ap_error_name(code)
                ),
            })
        } else {
            Err(AttentioError::Protocol {
                message: format!("unexpected response command: 0x{:02X}", resp.cmd),
            })
        }
    }

    // ── High-level commands ──────────────────────────────────────────────────

    /// Query device metadata (GET_METADATA 0x43).
    ///
    /// Returns a list of (key, value) pairs. The response payload uses the
    /// count-prefixed key-value format:
    /// `[count:1] { [key_len:1][key][val_len:1][val] } * count`
    pub async fn get_metadata(&mut self) -> Result<Vec<(String, String)>, AttentioError> {
        let payload = self.send_command_ok(CMD_GET_METADATA, &[]).await?;
        parse_kv_list(&payload)
    }

    /// List all settings (SETTINGS_LIST 0x50).
    ///
    /// Same count-prefixed key-value format as GET_METADATA.
    pub async fn settings_list(&mut self) -> Result<Vec<(String, String)>, AttentioError> {
        let payload = self.send_command_ok(CMD_SETTINGS_LIST, &[]).await?;
        parse_kv_list(&payload)
    }

    /// Get a single setting by key (SETTINGS_GET 0x51).
    ///
    /// Request payload: `[key_len:1][key]`
    /// Response payload: `[key_len:1][key][val_len:1][val]` (single pair, no count).
    pub async fn settings_get(&mut self, key: &str) -> Result<(String, String), AttentioError> {
        let mut req = Vec::with_capacity(1 + key.len());
        req.push(key.len() as u8);
        req.extend_from_slice(key.as_bytes());

        let payload = self.send_command_ok(CMD_SETTINGS_GET, &req).await?;
        parse_kv_single(&payload)
    }

    /// Set a setting (SETTINGS_SET 0x52).
    ///
    /// Request payload: `[key_len:1][key][val_len:1][val]`
    /// Response: bare OK (no payload).
    pub async fn settings_set(&mut self, key: &str, value: &str) -> Result<(), AttentioError> {
        let mut req = Vec::with_capacity(2 + key.len() + value.len());
        req.push(key.len() as u8);
        req.extend_from_slice(key.as_bytes());
        req.push(value.len() as u8);
        req.extend_from_slice(value.as_bytes());

        self.send_command_ok(CMD_SETTINGS_SET, &req).await?;
        Ok(())
    }
}

// ── Binary key-value payload parsing ─────────────────────────────────────────

/// Parse a count-prefixed key-value list from an AP response payload.
///
/// Format: `[count:1] { [key_len:1][key:key_len][val_len:1][val:val_len] } * count`
fn parse_kv_list(data: &[u8]) -> Result<Vec<(String, String)>, AttentioError> {
    if data.is_empty() {
        return Err(AttentioError::Protocol {
            message: "empty key-value list payload".to_string(),
        });
    }

    let count = data[0] as usize;
    let mut pos = 1;
    let mut entries = Vec::with_capacity(count);

    for i in 0..count {
        let (key, value, new_pos) =
            parse_kv_pair(data, pos).map_err(|msg| AttentioError::Protocol {
                message: format!("key-value list entry {}/{}: {}", i + 1, count, msg),
            })?;
        entries.push((key, value));
        pos = new_pos;
    }

    Ok(entries)
}

/// Parse a single key-value pair (no count prefix) from an AP response payload.
///
/// Format: `[key_len:1][key:key_len][val_len:1][val:val_len]`
fn parse_kv_single(data: &[u8]) -> Result<(String, String), AttentioError> {
    if data.is_empty() {
        return Err(AttentioError::Protocol {
            message: "empty key-value payload".to_string(),
        });
    }

    let (key, value, _) =
        parse_kv_pair(data, 0).map_err(|msg| AttentioError::Protocol { message: msg })?;
    Ok((key, value))
}

/// Parse one `[key_len][key][val_len][val]` pair starting at `pos`.
///
/// Returns `(key, value, new_pos)` on success, or an error message string.
fn parse_kv_pair(data: &[u8], pos: usize) -> Result<(String, String, usize), String> {
    if pos >= data.len() {
        return Err("unexpected end of data (no key_len)".to_string());
    }
    let key_len = data[pos] as usize;
    let key_start = pos + 1;
    let key_end = key_start + key_len;
    if key_end > data.len() {
        return Err(format!(
            "key_len={} exceeds remaining data ({} bytes)",
            key_len,
            data.len() - key_start
        ));
    }
    let key = String::from_utf8(data[key_start..key_end].to_vec())
        .map_err(|_| "key is not valid UTF-8".to_string())?;

    if key_end >= data.len() {
        return Err("unexpected end of data (no val_len)".to_string());
    }
    let val_len = data[key_end] as usize;
    let val_start = key_end + 1;
    let val_end = val_start + val_len;
    if val_end > data.len() {
        return Err(format!(
            "val_len={} exceeds remaining data ({} bytes)",
            val_len,
            data.len() - val_start
        ));
    }
    let value = String::from_utf8(data[val_start..val_end].to_vec())
        .map_err(|_| "value is not valid UTF-8".to_string())?;

    Ok((key, value, val_end))
}
