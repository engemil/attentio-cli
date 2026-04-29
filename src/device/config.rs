/// USB device identification constants for Attentio devices
///
/// This module centralizes all USB VID/PID and product string identifiers
/// for easy maintenance and future updates.

/// USB Vendor ID for Attentio devices (pid.codes open-source VID)
/// Granted via: https://pid.codes/1209/EEA1/
pub const ATTENTIO_VID: u16 = 0x1209;

/// USB Product ID for Attentio devices (AttentioLight-1)
/// Granted via: https://pid.codes/1209/EEA1/
pub const ATTENTIO_PID: u16 = 0xEEA1;

/// STM32 DFU fallback VID (STMicroelectronics).
/// Used by the bootloader when no valid application header is present
/// (blank/erased board).
pub const STM_DFU_VID: u16 = 0x0483;

/// STM32 DFU fallback PID.
/// Used by the bootloader when no valid application header is present
/// (blank/erased board).
pub const STM_DFU_PID: u16 = 0xDF11;

/// Product string patterns that indicate the device is in DFU/bootloader mode
pub const DFU_PRODUCT_PATTERNS: &[&str] = &["Bootloader", "DFU"];

/// Known product strings for devices in normal application mode
pub const APP_PRODUCT_STRINGS: &[&str] = &["AttentioLight-1", "Attentio"];

/// Check if a USB device matches an Attentio product VID/PID (pid.codes).
pub fn is_attentio_device(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == ATTENTIO_VID && product_id == ATTENTIO_PID
}

/// Check if a USB device matches the STM32 DFU fallback VID/PID.
/// This is used by the bootloader on blank/erased boards with no valid
/// application header.
pub fn is_stm_dfu_device(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == STM_DFU_VID && product_id == STM_DFU_PID
}

/// Check if a USB device is any known Attentio-related device
/// (either a product with pid.codes VID/PID or a blank board in STM32 DFU
/// fallback mode).
pub fn is_known_device(vendor_id: u16, product_id: u16) -> bool {
    is_attentio_device(vendor_id, product_id) || is_stm_dfu_device(vendor_id, product_id)
}
