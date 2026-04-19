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

/// Check if a USB device descriptor matches the Attentio VID/PID.
pub fn is_attentio_device(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == ATTENTIO_VID && product_id == ATTENTIO_PID
}
