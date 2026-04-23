use std::collections::HashMap;

use rusb::UsbContext;
use serde::Serialize;
use serialport::{SerialPortType, UsbPortInfo};
use tracing::{debug, trace, warn};

use super::config::{self, ATTENTIO_PID, ATTENTIO_VID};
use crate::error::AttentioError;
use crate::protocol::ApClient;

/// Represents the operational mode of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceMode {
    /// Device is running normal application firmware.
    Normal,
    /// Device is in DFU/bootloader mode.
    Bootloader,
    /// Device mode could not be determined.
    Unknown,
}

impl std::fmt::Display for DeviceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceMode::Normal => write!(f, "Normal"),
            DeviceMode::Bootloader => write!(f, "Bootloader"),
            DeviceMode::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Represents a CDC port role on a device.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CdcRole {
    /// Serial print stream (read-only).
    SerialPrints,
    /// Attentio Protocol (AP) interface.
    Protocol,
    /// Single CDC — role is ambiguous (pre-dual-CDC firmware).
    Single,
}

impl std::fmt::Display for CdcRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CdcRole::SerialPrints => write!(f, "serial"),
            CdcRole::Protocol => write!(f, "protocol"),
            CdcRole::Single => write!(f, "serial"),
        }
    }
}

/// A CDC port associated with a device.
#[derive(Debug, Clone, Serialize)]
pub struct CdcPort {
    /// System port path (e.g. /dev/ttyACM0).
    pub path: String,
    /// Role of this CDC interface.
    pub role: CdcRole,
}

/// A discovered device, potentially with multiple CDC ports.
#[derive(Debug, Clone, Serialize)]
pub struct AttentioDevice {
    /// USB serial number (chip UID from iSerialNumber descriptor, 24 hex chars).
    pub serial: String,
    /// USB product string (iProduct descriptor) — e.g. "EngEmil.io AttentioLight-1".
    pub device_type: Option<String>,
    /// User-assigned device name from persistent settings (e.g. "AttentioLight-1").
    #[serde(rename = "device_name")]
    pub product: Option<String>,
    /// Operational mode (normal application or bootloader/DFU).
    #[serde(rename = "status")]
    pub mode: DeviceMode,
    /// USB bus location string (e.g. "Bus 001 Device 060") for physical identification.
    pub usb_location: Option<String>,
    /// CDC0 port — serial prints (None if only single CDC).
    pub cdc0: Option<CdcPort>,
    /// CDC1 port — Attentio Protocol interface (None if only single CDC).
    pub cdc1: Option<CdcPort>,
    /// Single CDC port (used when firmware has only one CDC interface).
    pub single_cdc: Option<CdcPort>,
}

impl AttentioDevice {
    /// Returns the AP (Attentio Protocol) port path (CDC1 if dual, single CDC otherwise).
    pub fn ap_port(&self) -> Option<&str> {
        self.cdc1
            .as_ref()
            .or(self.single_cdc.as_ref())
            .map(|p| p.path.as_str())
    }

    /// Returns the serial prints port path (CDC0 if dual, None otherwise).
    pub fn serial_port(&self) -> Option<&str> {
        self.cdc0.as_ref().map(|p| p.path.as_str())
    }

    /// Returns all port paths for this device.
    pub fn all_ports(&self) -> Vec<&CdcPort> {
        let mut ports = Vec::new();
        if let Some(ref p) = self.cdc0 {
            ports.push(p);
        }
        if let Some(ref p) = self.cdc1 {
            ports.push(p);
        }
        if let Some(ref p) = self.single_cdc {
            ports.push(p);
        }
        ports
    }
}

/// Raw USB port info extracted from serialport enumeration.
#[derive(Debug)]
struct RawUsbPort {
    path: String,
    info: UsbPortInfo,
}

/// Detect the device mode based on USB product string.
///
/// Checks the product string against known patterns to determine if the device
/// is in normal application mode, DFU/bootloader mode, or unknown state.
fn detect_device_mode(product: Option<&String>) -> DeviceMode {
    let Some(product) = product else {
        return DeviceMode::Unknown;
    };

    // Check for DFU/bootloader mode indicators
    for pattern in config::DFU_PRODUCT_PATTERNS {
        if product.contains(pattern) {
            return DeviceMode::Bootloader;
        }
    }

    // Check for known application mode product strings
    for app_product in config::APP_PRODUCT_STRINGS {
        if product.contains(app_product) {
            return DeviceMode::Normal;
        }
    }

    DeviceMode::Unknown
}

/// Discover all connected devices.
///
/// Enumerates serial ports, filters by VID/PID, groups by USB serial number
/// (from the iSerialNumber descriptor — the chip's unique 96-bit UID),
/// and classifies CDC ports (dual CDC: lower port number = CDC0, higher = CDC1).
/// Also detects pure DFU devices that don't expose serial ports.
///
/// For normal-mode devices with an AP protocol port, queries the `device_name`
/// setting to populate the `product` field.
pub async fn find_devices() -> Result<Vec<AttentioDevice>, AttentioError> {
    let mut devices = find_devices_fast()?;

    // Query device_name from each normal-mode device via AP protocol.
    for device in &mut devices {
        if device.mode != DeviceMode::Normal {
            continue;
        }
        let Some(port_path) = device.ap_port().map(|s| s.to_string()) else {
            continue;
        };
        match query_device_name(&port_path).await {
            Ok(name) => {
                debug!("Device {} name: {}", device.serial, name);
                device.product = Some(name);
            }
            Err(e) => {
                debug!(
                    "Failed to query device_name for {} on {}: {}",
                    device.serial, port_path, e
                );
            }
        }
    }

    Ok(devices)
}

/// Lightweight device discovery without opening serial ports.
///
/// Same as [`find_devices()`] but skips querying device settings (device name)
/// over the shell port. Useful when only the device's presence, mode, and
/// serial number are needed (e.g., polling for device re-enumeration after
/// DFU flash).
pub fn find_devices_fast() -> Result<Vec<AttentioDevice>, AttentioError> {
    let ports = serialport::available_ports().map_err(AttentioError::Serial)?;

    debug!("Found {} total serial ports (fast)", ports.len());
    trace!("All ports: {:#?}", ports);

    let mut devices = devices_from_ports(ports);

    // Also check for DFU-only (Bootloader) devices (no serial ports)
    match find_dfu_only_devices() {
        Ok(dfu_devices) => {
            debug!("Found {} DFU-only devices", dfu_devices.len());
            devices.extend(dfu_devices);
        }
        Err(e) => {
            warn!("Failed to enumerate USB devices for DFU detection: {}", e);
        }
    }

    // Sort by serial for deterministic output
    devices.sort_by(|a, b| a.serial.cmp(&b.serial));

    Ok(devices)
}

/// Find devices that are in pure DFU mode (Bootloader) (no CDC serial ports).
///
/// Uses libusb to enumerate USB devices and find those with matching VID/PID
/// that expose a DFU interface but no serial ports.
fn find_dfu_only_devices() -> Result<Vec<AttentioDevice>, AttentioError> {
    let context = rusb::Context::new()
        .map_err(|e| AttentioError::Other(format!("failed to create USB context: {}", e)))?;

    let devices = context
        .devices()
        .map_err(|e| AttentioError::Other(format!("failed to enumerate USB devices: {}", e)))?;

    let mut dfu_devices = Vec::new();

    for device in devices.iter() {
        let Ok(desc) = device.device_descriptor() else {
            continue;
        };

        // Check if this is our device
        if !config::is_attentio_device(desc.vendor_id(), desc.product_id()) {
            continue;
        }

        // Try to open the device to read strings to detect mode
        let (mode, device_type, serial) = if let Ok(handle) = device.open() {
            let product = handle.read_product_string_ascii(&desc).ok();
            let serial = handle.read_serial_number_string_ascii(&desc).ok();
            let mode = detect_device_mode(product.as_ref());
            (mode, product, serial)
        } else {
            (DeviceMode::Unknown, None, None)
        };

        // Only add this device if it's in bootloader mode
        // (normal mode devices should be detected via serial ports)
        if mode == DeviceMode::Bootloader {
            let usb_location = format!(
                "Bus {:03} Device {:03}",
                device.bus_number(),
                device.address()
            );
            debug!("Found DFU device in bootloader mode at {}", usb_location);

            let serial = serial.unwrap_or_else(|| "unknown".to_string());
            debug!("DFU device serial: {}", serial);

            dfu_devices.push(AttentioDevice {
                serial,
                device_type,
                product: None,
                mode,
                usb_location: Some(usb_location),
                cdc0: None,
                cdc1: None,
                single_cdc: None,
            });
        }
    }

    Ok(dfu_devices)
}

/// USB device info read via libusb (serial, product string, and bus location).
struct UsbDeviceInfo {
    serial: Option<String>,
    product: Option<String>,
    location: String,
}

/// Read USB device info (iSerialNumber, iProduct string, and bus location)
/// for a device identified by its USB serial number, or — if the serial is
/// unknown — by being the sole device with matching VID/PID.
///
/// `serialport`'s port enumeration often returns `product: None` and
/// `serial_number: None` on Linux because it doesn't open the device to read
/// string descriptors. We use `rusb` directly to read the iSerialNumber and
/// iProduct descriptors, plus the bus/device address.
fn read_usb_device_info(usb_serial: &str) -> Option<UsbDeviceInfo> {
    let context = rusb::Context::new().ok()?;
    let devices = context.devices().ok()?;

    let mut candidates: Vec<UsbDeviceInfo> = Vec::new();

    for device in devices.iter() {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if !config::is_attentio_device(desc.vendor_id(), desc.product_id()) {
            continue;
        }

        let location = format!(
            "Bus {:03} Device {:03}",
            device.bus_number(),
            device.address()
        );

        let handle = match device.open() {
            Ok(h) => h,
            Err(e) => {
                debug!("read_usb_device_info: cannot open device: {}", e);
                continue;
            }
        };

        let serial = handle.read_serial_number_string_ascii(&desc).ok();
        let product = handle.read_product_string_ascii(&desc).ok();

        if usb_serial != "unknown" {
            // Match by serial number string — skip if it doesn't match
            match &serial {
                Some(s) if s == usb_serial => {
                    return Some(UsbDeviceInfo {
                        serial,
                        product,
                        location,
                    });
                }
                _ => continue,
            }
        } else {
            // Serial unknown — collect all candidates and return if unambiguous
            candidates.push(UsbDeviceInfo {
                serial,
                product,
                location,
            });
        }
    }

    // Only use the fallback if there's exactly one Attentio device connected
    if candidates.len() == 1 {
        debug!(
            "read_usb_device_info: serial unknown, found 1 candidate: {:?}",
            candidates[0].product
        );
        Some(candidates.into_iter().next().unwrap())
    } else {
        debug!(
            "read_usb_device_info: serial unknown, {} candidates — ambiguous, skipping",
            candidates.len()
        );
        None
    }
}

/// Query the `device_name` setting from a device via AP protocol.
///
/// Opens a short-lived connection to the device's AP port, sends a
/// SETTINGS_GET("device_name") command, and returns the value.
async fn query_device_name(port_path: &str) -> Result<String, AttentioError> {
    // Brief delay to let CDC ACM link settle.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = ApClient::open(port_path)?;
    let (_key, value) = client.settings_get("device_name").await?;
    Ok(value)
}

/// Build device list from raw serial port info.
///
/// Filters by VID/PID, groups by serial number, and classifies CDC ports.
/// Separated from `find_devices` to allow unit testing with mock port data.
pub fn devices_from_ports(ports: Vec<serialport::SerialPortInfo>) -> Vec<AttentioDevice> {
    // Filter ports for attentio related device(s) only
    let attentio_ports: Vec<RawUsbPort> = ports
        .into_iter()
        .filter_map(|port| match port.port_type {
            SerialPortType::UsbPort(ref usb_info)
                if usb_info.vid == ATTENTIO_VID && usb_info.pid == ATTENTIO_PID =>
            {
                Some(RawUsbPort {
                    path: port.port_name.clone(),
                    info: usb_info.clone(),
                })
            }
            _ => None,
        })
        .collect();

    debug!("Found {} serial ports", attentio_ports.len());

    if attentio_ports.is_empty() {
        return Vec::new();
    }

    // Group ports by serial number
    let mut by_serial: HashMap<String, Vec<RawUsbPort>> = HashMap::new();
    for port in attentio_ports {
        let serial = port
            .info
            .serial_number
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_serial.entry(serial).or_default().push(port);
    }

    // Build AttentioDevice for each unique serial
    let mut devices: Vec<AttentioDevice> = Vec::new();

    for (usb_serial, mut ports) in by_serial {
        // Sort by port path so lower index = CDC0, higher = CDC1
        ports.sort_by(|a, b| a.path.cmp(&b.path));

        // Try to detect mode from USB product string if available
        // But we'll assume Normal mode if device has CDC ports
        let product_str = ports.first().and_then(|p| p.info.product.as_ref());
        let mode = detect_device_mode(product_str);

        // If mode is Unknown but device has CDC ports, assume Normal mode
        // (DFU devices don't expose CDC serial ports)
        let mode = if mode == DeviceMode::Unknown {
            DeviceMode::Normal
        } else {
            mode
        };

        // Read USB device info (iSerialNumber, iProduct string + bus location)
        // via libusb — serialport often returns None for product and serial on
        // Linux because it doesn't open the device to read descriptors.
        let usb_info = read_usb_device_info(&usb_serial);
        let device_type = usb_info.as_ref().and_then(|i| i.product.clone());
        let usb_location = usb_info.as_ref().map(|i| i.location.clone());
        debug!("Device type from USB descriptor: {:?}", device_type);
        debug!("USB location: {:?}", usb_location);

        // Determine the device serial: prefer the serial from serialport
        // (USB descriptor), fall back to rusb reading, then "unknown".
        let serial = if usb_serial != "unknown" {
            usb_serial.clone()
        } else if let Some(rusb_serial) = usb_info.and_then(|i| i.serial) {
            debug!(
                "Using rusb fallback serial: {} (serialport returned None)",
                rusb_serial
            );
            rusb_serial
        } else {
            "unknown".to_string()
        };

        let device = if ports.len() >= 2 {
            // Dual CDC: first port = CDC0 (serial prints), second = CDC1 (protocol)
            debug!(
                "Device: dual CDC — CDC0={}, CDC1={} — serial={} — mode={}",
                ports[0].path, ports[1].path, serial, mode
            );
            AttentioDevice {
                serial,
                device_type,
                product: None,
                mode,
                usb_location,
                cdc0: Some(CdcPort {
                    path: ports[0].path.clone(),
                    role: CdcRole::SerialPrints,
                }),
                cdc1: Some(CdcPort {
                    path: ports[1].path.clone(),
                    role: CdcRole::Protocol,
                }),
                single_cdc: None,
            }
        } else {
            // Single CDC
            debug!(
                "Device: single CDC — {} — serial={} — mode={}",
                ports[0].path, serial, mode
            );
            AttentioDevice {
                serial,
                device_type,
                product: None,
                mode,
                usb_location,
                cdc0: None,
                cdc1: None,
                single_cdc: Some(CdcPort {
                    path: ports[0].path.clone(),
                    role: CdcRole::Single,
                }),
            }
        };

        devices.push(device);
    }

    debug!("Discovered {} device(s)", devices.len());
    devices
}

/// Find a specific device by serial number or index, or the only connected device.
///
/// The `target` parameter accepts:
///   - A device index (1-based number matching the `#` column from `attentio list`)
///   - A serial number string (exact match)
///   - None: auto-selects if exactly one device is connected
pub async fn resolve_device(target: Option<&str>) -> Result<AttentioDevice, AttentioError> {
    let devices = find_devices().await?;
    select_device(devices, target)
}

/// Select a device from a list by serial number, index, or return the only one.
///
/// If `target` is a small positive integer (1-based), it is treated as a device
/// index into the sorted device list (matching the `#` column from `attentio list`).
/// Otherwise, it is treated as an exact serial number match.
///
/// Separated from `resolve_device` to allow unit testing without hardware.
pub fn select_device(
    devices: Vec<AttentioDevice>,
    target: Option<&str>,
) -> Result<AttentioDevice, AttentioError> {
    match target {
        Some(target) => {
            // If the target parses as a positive integer, treat it as a 1-based device index.
            // Serial numbers are 24-char hex strings (e.g. "3A002B000F51363439373834"),
            // so they will never parse as a small usize.
            if let Ok(index) = target.parse::<usize>() {
                let count = devices.len();
                if index == 0 || index > count {
                    return Err(AttentioError::DeviceIndexOutOfRange { index, count });
                }
                // 1-based index: device #1 is devices[0]
                Ok(devices.into_iter().nth(index - 1).unwrap())
            } else {
                // Not a number — treat as serial number (exact match)
                devices
                    .into_iter()
                    .find(|d| d.serial == target)
                    .ok_or_else(|| AttentioError::DeviceSerialNotFound {
                        serial: target.to_string(),
                    })
            }
        }
        None => match devices.len() {
            0 => Err(AttentioError::DeviceNotFound),
            1 => Ok(devices.into_iter().next().unwrap()),
            _ => {
                let serials = devices
                    .iter()
                    .map(|d| d.serial.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(AttentioError::MultipleDevices { serials })
            }
        },
    }
}
