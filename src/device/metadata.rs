//! Device metadata query module.
//!
//! Provides a structured query helper for the firmware's `metadata` shell
//! command over an already-opened connection. Currently unused — the CLI's
//! `metadata` subcommand sends raw shell commands directly. Kept for
//! potential future use.
//!
//! Note: Device serial number is now obtained from the USB iSerialNumber
//! descriptor (chip UID), not from this module.

#![allow(dead_code)]

use tracing::debug;

use crate::device::connection::DeviceConnection;
use crate::error::AttentioError;

/// Device metadata retrieved from the firmware's `metadata` command.
#[derive(Debug, Clone)]
pub struct DeviceMetadata {
    /// Device serial number (production-programmed, read-only).
    pub serial_number: Option<String>,
}

/// Query device metadata from an already-opened and synced connection.
///
/// Sends `metadata get serial_number` and parses the response.
pub async fn query_device_metadata(
    conn: &mut DeviceConnection,
) -> Result<DeviceMetadata, AttentioError> {
    let mut serial_number = None;

    match conn.send_command("metadata get serial_number").await {
        Ok(response) => {
            let value = parse_response_value(&response, "serial_number");
            if let Some(v) = value {
                debug!("Found serial_number: {}", v);
                serial_number = Some(v);
            }
        }
        Err(e) => {
            debug!("Failed to query serial_number from metadata: {}", e);
        }
    }

    Ok(DeviceMetadata { serial_number })
}

/// Parse a `key=value` response line, returning the value if the key matches.
/// Returns `None` if the response is empty or doesn't match the expected format.
fn parse_response_value(response: &str, expected_key: &str) -> Option<String> {
    let trimmed = response.trim();
    if let Some((key, value)) = trimmed.split_once('=') {
        if key == expected_key {
            return Some(value.to_string());
        }
    }
    None
}
