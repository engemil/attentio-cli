use std::collections::HashMap;

use serde::Serialize;
use serialport::{SerialPortType, UsbPortInfo};
use tracing::{debug, trace};

use crate::error::AttentioError;

/// AttentioLight-1 USB Vendor ID (STMicroelectronics / EngEmil.io).
pub const ATTENTIO_VID: u16 = 0x0483;

/// AttentioLight-1 USB Product ID.
pub const ATTENTIO_PID: u16 = 0xDF11;

/// Represents a CDC port role on a device.
#[derive(Debug, Clone, Serialize)]
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
            CdcRole::DebugPrints => write!(f, "CDC0 (debug)"),
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
    /// USB serial number (unique per device).
    pub serial: String,
    /// USB manufacturer string.
    pub manufacturer: Option<String>,
    /// USB product string.
    pub product: Option<String>,
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
    #[allow(dead_code)] // Used in Phase 3 (monitor)
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

/// Discover all connected devices.
///
/// Enumerates serial ports, filters by VID/PID, groups by serial number,
/// and classifies CDC ports (dual CDC: lower port number = CDC0, higher = CDC1).
pub fn find_devices() -> Result<Vec<AttentioDevice>, AttentioError> {
    let ports = serialport::available_ports().map_err(AttentioError::Serial)?;

    debug!("Found {} total serial ports", ports.len());
    trace!("All ports: {:#?}", ports);

    Ok(devices_from_ports(ports))
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

    debug!(
        "Found {} serial ports",
        attentio_ports.len()
    );

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

    for (serial, mut ports) in by_serial {
        // Sort by port path so lower index = CDC0, higher = CDC1
        ports.sort_by(|a, b| a.path.cmp(&b.path));

        let manufacturer = ports.first().and_then(|p| p.info.manufacturer.clone());
        let product = ports.first().and_then(|p| p.info.product.clone());

        let device = if ports.len() >= 2 {
            // Dual CDC: first port = CDC0 (debug), second = CDC1 (shell)
            debug!(
                "Device {}: dual CDC — CDC0={}, CDC1={}",
                serial, ports[0].path, ports[1].path
            );
            AttentioDevice {
                serial,
                manufacturer,
                product,
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
            debug!("Device {}: single CDC — {}", serial, ports[0].path);
            AttentioDevice {
                serial,
                manufacturer,
                product,
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
pub fn resolve_device(serial: Option<&str>) -> Result<AttentioDevice, AttentioError> {
    let devices = find_devices()?;
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
