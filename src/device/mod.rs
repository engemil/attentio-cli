pub mod config;
pub mod connection;
pub mod discovery;

// Re-export commonly used types
#[allow(unused_imports)]
pub use discovery::{AttentioDevice, CdcPort, CdcRole, DeviceMode};
