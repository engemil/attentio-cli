/// USB device identification constants for Attentio devices
///
/// This module centralizes all USB VID/PID and product string identifiers
/// for easy maintenance and future updates.

/// USB Vendor ID for Attentio devices (STMicroelectronics / EngEmil.io)
pub const ATTENTIO_VID: u16 = 0x0483;

/// USB Product ID for Attentio devices
pub const ATTENTIO_PID: u16 = 0xDF11;

/// Product string patterns that indicate the device is in DFU/bootloader mode
pub const DFU_PRODUCT_PATTERNS: &[&str] = &["Bootloader", "DFU"];

/// Known product strings for devices in normal application mode
pub const APP_PRODUCT_STRINGS: &[&str] = &["AttentioLight-1", "Attentio"];
