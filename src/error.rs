use thiserror::Error;

/// Attentio CLI error types.
#[derive(Debug, Error)]
pub enum AttentioError {
    #[error("no device(s) found")]
    DeviceNotFound,

    #[error("multiple devices found — use --device <serial> to select one: {serials}")]
    MultipleDevices { serials: String },

    #[error("device with serial '{serial}' not found")]
    DeviceSerialNotFound { serial: String },

    #[error("serial port error: {0}")]
    Serial(#[from] serialport::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol error: {message}")]
    Protocol { message: String },

    #[error("command timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("{0}")]
    Other(String),
}
