pub mod config;
pub mod connection;
pub mod discovery;
pub mod metadata;
pub mod settings;

// Re-export commonly used types
#[allow(unused_imports)]
pub use discovery::{AttentioDevice, CdcPort, CdcRole, DeviceMode};
