use std::collections::HashMap;
use std::time::Duration;

use rusb::UsbContext;
use serde::Serialize;
use serialport::{SerialPortType, UsbPortInfo};
use tracing::{debug, trace, warn};

use super::config::{self, ATTENTIO_PID, ATTENTIO_VID};
use super::connection::DeviceConnection;
use crate::error::AttentioError;

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
    /// Debug print stream (read-only).
    DebugPrints,
    /// Shell command interface (request/response).
    Shell,
    /// Single CDC — role is ambiguous (pre-dual-CDC firmware).
    Single,
}

impl std::fmt::Display for CdcRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CdcRole::DebugPrints => write!(f, "CDC0 (debug_prints)"),
            CdcRole::Shell => write!(f, "CDC1 (shell)"),
            CdcRole::Single => write!(f, "single"),
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
    /// CDC0 port — debug prints (None if only single CDC).
    pub cdc0: Option<CdcPort>,
    /// CDC1 port — shell commands (None if only single CDC).
    pub cdc1: Option<CdcPort>,
    /// Single CDC port (used when firmware has only one CDC interface).
    pub single_cdc: Option<CdcPort>,
}

impl AttentioDevice {
    /// Returns the shell port path (CDC1 if dual, single CDC otherwise).
    pub fn shell_port(&self) -> Option<&str> {
        self.cdc1
            .as_ref()
            .or(self.single_cdc.as_ref())
            .map(|p| p.path.as_str())
    }

    /// Returns the debug prints port path (CDC0 if dual, None otherwise).
    pub fn debug_port(&self) -> Option<&str> {
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
/// For devices in Normal mode with shell ports, queries device settings
/// (device name) from the device.
pub async fn find_devices() -> Result<Vec<AttentioDevice>, AttentioError> {
    let ports = serialport::available_ports().map_err(AttentioError::Serial)?;

    debug!("Found {} total serial ports", ports.len());
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

    // Query settings from Normal mode devices (sequentially)
    for device in &mut devices {
        if device.mode == DeviceMode::Normal {
            if let Some(shell_port) = device.shell_port().map(|s| s.to_string()) {
                query_device_info(device, &shell_port).await;
            }
        }
    }

    // Sort by serial for deterministic output
    devices.sort_by(|a, b| a.serial.cmp(&b.serial));

    Ok(devices)
}

/// Query device settings from a device over its shell port.
///
/// Opens a single connection, syncs the shell, then queries settings
/// (device name). Serial number is no longer queried here — it comes
/// from the USB iSerialNumber descriptor. Retries once on failure
/// with 250ms backoff.
async fn query_device_info(device: &mut AttentioDevice, shell_port: &str) {
    const MAX_ATTEMPTS: usize = 2;
    const RETRY_DELAY: Duration = Duration::from_millis(250);

    for attempt in 1..=MAX_ATTEMPTS {
        debug!(
            "Querying device info from {} (attempt {}/{})",
            shell_port, attempt, MAX_ATTEMPTS
        );

        if attempt > 1 {
            debug!("Waiting {}ms before retry...", RETRY_DELAY.as_millis());
            tokio::time::sleep(RETRY_DELAY).await;
        }

        // Open connection
        let mut conn = match DeviceConnection::open(shell_port) {
            Ok(c) => c,
            Err(e) => {
                if matches!(e, AttentioError::PortBusy { .. }) {
                    debug!("Port busy on {}, device starting up", shell_port);
                    return;
                }
                debug!("Failed to open {}: {}", shell_port, e);
                if attempt == MAX_ATTEMPTS {
                    warn!(
                        "Failed to open {} after {} attempts: {}",
                        shell_port, MAX_ATTEMPTS, e
                    );
                }
                continue;
            }
        };

        // Sync shell
        if let Err(e) = conn.sync_shell().await {
            debug!("Failed to sync shell on {}: {}", shell_port, e);
            if attempt == MAX_ATTEMPTS {
                warn!(
                    "Failed to sync shell on {} after {} attempts: {}",
                    shell_port, MAX_ATTEMPTS, e
                );
            }
            continue;
        }

        // Query settings (device name)
        match crate::device::settings::query_device_settings(&mut conn).await {
            Ok(settings) => {
                if let Some(name) = settings.device_name {
                    device.product = Some(name);
                }
            }
            Err(e) => {
                debug!("Failed to query settings from {}: {}", shell_port, e);
            }
        }

        // Success — got at least a connection, no need to retry
        debug!("Successfully queried device info on attempt {}", attempt);
        return;
    }
}

/// Find devices that are in pure DFU mode (Bootloader) (no CDC serial ports).
///
/// Uses libusb to enumerate USB devices and find those with matching VID/PID
/// that expose a DFU interface but no serial ports.
fn find_dfu_only_devices() -> Result<Vec<AttentioDevice>, String> {
    let Ok(context) = rusb::Context::new() else {
        return Err("Failed to create USB context".to_string());
    };

    let Ok(devices) = context.devices() else {
        return Err("Failed to enumerate USB devices".to_string());
    };

    let mut dfu_devices = Vec::new();

    for device in devices.iter() {
        let Ok(desc) = device.device_descriptor() else {
            continue;
        };

        // Check if this is our device
        if desc.vendor_id() != ATTENTIO_VID || desc.product_id() != ATTENTIO_PID {
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

        if desc.vendor_id() != ATTENTIO_VID || desc.product_id() != ATTENTIO_PID {
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
            // Dual CDC: first port = CDC0 (debug), second = CDC1 (shell)
            debug!(
                "Device: dual CDC — CDC0={}, CDC1={} — serial={} — mode={}",
                ports[0].path, ports[1].path, serial, mode
            );
            AttentioDevice {
                serial,
                device_type,
                product: None, // Will be queried from settings
                mode,
                usb_location,
                cdc0: Some(CdcPort {
                    path: ports[0].path.clone(),
                    role: CdcRole::DebugPrints,
                }),
                cdc1: Some(CdcPort {
                    path: ports[1].path.clone(),
                    role: CdcRole::Shell,
                }),
                single_cdc: None,
            }
        } else {
            // Single CDC: treat as shell port
            debug!(
                "Device: single CDC — {} — serial={} — mode={}",
                ports[0].path, serial, mode
            );
            AttentioDevice {
                serial,
                device_type,
                product: None, // Will be queried from settings
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

    // Sort devices by serial for deterministic output
    devices.sort_by(|a, b| a.serial.cmp(&b.serial));

    debug!("Discovered {} device(s)", devices.len());
    devices
}

/// Find a specific device by serial number, or the only connected device.
///
/// If `serial` is Some, filters to that device.
/// If `serial` is None:
///   - Returns the device if exactly one is connected.
///   - Returns an error if zero or multiple devices are connected.
pub async fn resolve_device(serial: Option<&str>) -> Result<AttentioDevice, AttentioError> {
    let devices = find_devices().await?;
    select_device(devices, serial)
}

/// Select a device from a list by serial number, or return the only one.
///
/// Separated from `resolve_device` to allow unit testing without hardware.
pub fn select_device(
    devices: Vec<AttentioDevice>,
    serial: Option<&str>,
) -> Result<AttentioDevice, AttentioError> {
    match serial {
        Some(target) => devices
            .into_iter()
            .find(|d| d.serial == target)
            .ok_or_else(|| AttentioError::DeviceSerialNotFound {
                serial: target.to_string(),
            }),
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
