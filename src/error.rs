use serde_json::json;
use thiserror::Error;

/// Attentio CLI error types.
#[derive(Debug, Error)]
pub enum AttentioError {
    #[error("no device(s) found")]
    DeviceNotFound,

    #[error("multiple devices found — use --device <#> or --device <serial> to select one (run 'attentio list' to see devices): {serials}")]
    MultipleDevices { serials: String },

    #[error("device with serial '{serial}' not found")]
    DeviceSerialNotFound { serial: String },

    #[error("device #{index} not found — only {count} device(s) connected. Run 'attentio list' to see available devices.")]
    DeviceIndexOutOfRange { index: usize, count: usize },

    #[error("port {port} is busy — another process has it open")]
    PortBusy { port: String },

    #[error("serial port error: {0}")]
    Serial(#[from] serialport::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol error: {message}")]
    Protocol { message: String },

    #[error("command timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("BLE error: {0}")]
    Ble(String),

    #[error("no BLE device found matching {selector}")]
    BleNotFound { selector: String },

    #[error("BLE pairing/bonding failed: {0}")]
    BlePairing(String),

    #[error("{0}")]
    Other(String),
}

impl AttentioError {
    /// Returns true if this error indicates the port is busy (held by another process).
    pub fn is_port_busy(&self) -> bool {
        matches!(self, AttentioError::PortBusy { .. })
    }

    /// Returns the error type as a string for JSON output.
    pub fn error_type(&self) -> &str {
        match self {
            AttentioError::DeviceNotFound => "DeviceNotFound",
            AttentioError::MultipleDevices { .. } => "MultipleDevices",
            AttentioError::DeviceSerialNotFound { .. } => "DeviceSerialNotFound",
            AttentioError::DeviceIndexOutOfRange { .. } => "DeviceIndexOutOfRange",
            AttentioError::PortBusy { .. } => "PortBusy",
            AttentioError::Serial(_) => "Serial",
            AttentioError::Io(_) => "Io",
            AttentioError::Protocol { .. } => "Protocol",
            AttentioError::Timeout { .. } => "Timeout",
            AttentioError::Ble(_) => "Ble",
            AttentioError::BleNotFound { .. } => "BleNotFound",
            AttentioError::BlePairing(_) => "BlePairing",
            AttentioError::Other(_) => "Other",
        }
    }

    /// Returns additional context data for JSON output.
    pub fn context_data(&self) -> serde_json::Value {
        match self {
            AttentioError::MultipleDevices { serials } => json!({
                "available_devices": serials.split(", ").collect::<Vec<_>>()
            }),
            AttentioError::DeviceSerialNotFound { serial } => json!({
                "requested_serial": serial
            }),
            AttentioError::DeviceIndexOutOfRange { index, count } => json!({
                "requested_index": index,
                "device_count": count
            }),
            AttentioError::PortBusy { port } => json!({
                "port": port
            }),
            AttentioError::Timeout { seconds } => json!({
                "timeout_seconds": seconds
            }),
            AttentioError::Protocol { message } => json!({
                "protocol_message": message
            }),
            AttentioError::BleNotFound { selector } => json!({
                "selector": selector
            }),
            _ => json!({}),
        }
    }
}
