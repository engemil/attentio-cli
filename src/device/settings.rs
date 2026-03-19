//! Device settings query module
//!
//! Queries user-configurable settings (device name) via the `settings`
//! shell command over an already-opened connection.

use tracing::debug;

use crate::device::connection::DeviceConnection;
use crate::error::AttentioError;

/// Device settings retrieved from the firmware's `settings` command.
#[derive(Debug, Clone)]
pub struct DeviceSettings {
    /// User-assigned device name (read-write setting).
    pub device_name: Option<String>,
}

/// Query device settings from an already-opened and synced connection.
///
/// Sends `settings get device_name` and parses the response.
pub async fn query_device_settings(
    conn: &mut DeviceConnection,
) -> Result<DeviceSettings, AttentioError> {
    let mut device_name = None;

    match conn.send_command("settings get device_name").await {
        Ok(response) => {
            let value = response.trim().to_string();
            if !value.is_empty() {
                debug!("Found device_name: {}", value);
                device_name = Some(value);
            }
        }
        Err(e) => {
            debug!("Failed to query device_name from settings: {}", e);
        }
    }

    Ok(DeviceSettings { device_name })
}
