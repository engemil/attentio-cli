use std::collections::HashMap;
use std::io::{BufRead, IsTerminal, Write};
use std::sync::{Mutex, OnceLock};

use rusb::UsbContext;
use serde::Serialize;
use serialport::{SerialPortType, UsbPortInfo};
use tracing::{debug, trace, warn};

/// Process-local cache of the last successfully read `device_name` setting,
/// keyed by USB serial number.
///
/// Reading the `device_name` setting requires opening the AP port and doing a
/// round-trip over the serial protocol. That can fail transiently (port busy
/// from another concurrent caller, momentary I/O error, etc.), in which case
/// we'd otherwise return `product = None` and any UI consumer would briefly
/// flip to a fallback label. The cache lets us keep showing the last-known
/// good name across such blips. The cache is cleared by individual entries
/// only when explicitly invalidated; a process restart resets everything.
fn name_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Cache of the known CDC protocol port path per device serial number.
///
/// On Windows (and potentially other platforms), COM port numbers don't
/// reliably correspond to USB interface order. Once we've successfully
/// probed which port is the protocol port (CDC1), we cache that mapping
/// so subsequent discovery cycles don't need to re-probe — which would
/// fail when the ApClient already holds the port exclusively.
fn cdc_protocol_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remember the protocol port path for a device serial number.
fn cdc_protocol_cache_remember(serial: &str, port_path: &str) {
    if let Ok(mut g) = cdc_protocol_cache().lock() {
        g.insert(serial.to_string(), port_path.to_string());
    }
}

/// Return the cached protocol port path for a device serial number, if known.
fn cdc_protocol_cache_lookup(serial: &str) -> Option<String> {
    cdc_protocol_cache().lock().ok().and_then(|g| g.get(serial).cloned())
}

pub fn cache_remember(serial: &str, name: &str) {
    if let Ok(mut g) = name_cache().lock() {
        g.insert(serial.to_string(), name.to_string());
    }
}

fn cache_lookup(serial: &str) -> Option<String> {
    name_cache().lock().ok().and_then(|g| g.get(serial).cloned())
}

use super::config;
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

/// The transport over which a device was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// USB CDC-ACM serial.
    Usb,
    /// Bluetooth Low Energy.
    Ble,
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transport::Usb => write!(f, "USB"),
            Transport::Ble => write!(f, "BLE"),
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
    /// System port path (e.g. `/dev/ttyACM0` on Linux, `COM3` on Windows).
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
    /// Transport over which this device was discovered.
    pub transport: Transport,
    /// BLE address (BD_ADDR), present only for BLE-discovered devices.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ble_address: Option<String>,
    /// BlueZ pairing status for BLE devices (`Some(true|false)`); `None` for USB
    /// or when it can't be determined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired: Option<bool>,
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

    // On some platforms (notably Windows), COM port numbers don't correspond
    // to USB interface indices. Probe dual-CDC devices to determine which
    // port is the protocol port (CDC1) and which is serial prints (CDC0).
    probe_and_fix_cdc_roles(&mut devices).await;

    // Query device_name from all normal-mode devices in parallel via AP protocol.
    // Each device has its own independent CDC port, so parallel access is safe
    // and reduces total latency compared to sequential queries.
    let mut tasks: tokio::task::JoinSet<(usize, Result<String, AttentioError>)> =
        tokio::task::JoinSet::new();

    for (idx, device) in devices.iter().enumerate() {
        if device.mode != DeviceMode::Normal {
            continue;
        }
        let Some(port_path) = device.ap_port().map(|s| s.to_string()) else {
            continue;
        };
        tasks.spawn(async move { (idx, query_device_name(&port_path).await) });
    }

    while let Some(result) = tasks.join_next().await {
        let Ok((idx, name_result)) = result else {
            continue;
        };
        match name_result {
            Ok(name) => {
                debug!("Device {} name: {}", devices[idx].serial, name);
                cache_remember(&devices[idx].serial, &name);
                devices[idx].product = Some(name);
            }
            Err(e) => {
                debug!(
                    "Failed to query device_name for {}: {} — using cached value if any",
                    devices[idx].serial, e
                );
                if let Some(cached) = cache_lookup(&devices[idx].serial) {
                    devices[idx].product = Some(cached);
                }
            }
        }
    }

    Ok(devices)
}

/// Best-effort BLE scan duration for `list`.
const BLE_LIST_SCAN: std::time::Duration = std::time::Duration::from_secs(3);

/// Discover Attentio devices advertising over BLE, mapped to [`AttentioDevice`].
///
/// Best-effort: returns an empty list on a host without a BLE adapter so that
/// `attentio list` still works for USB-only setups.
pub async fn find_ble_devices() -> Vec<AttentioDevice> {
    let found = super::ble::scan(BLE_LIST_SCAN).await;
    let mut devices = Vec::with_capacity(found.len());
    for info in found {
        let paired = super::ble::paired_status(&info.address).await;
        devices.push(AttentioDevice {
            serial: info.address.clone(),
            device_type: Some("AttentioLight-1 (BLE)".to_string()),
            product: info.name,
            mode: DeviceMode::Normal,
            usb_location: None,
            cdc0: None,
            cdc1: None,
            single_cdc: None,
            transport: Transport::Ble,
            ble_address: Some(info.address),
            paired,
        });
    }
    devices
}

/// Enumerate all devices across transports: USB first, then BLE.
///
/// Used by `attentio list`. The per-command path (`resolve_device`) stays
/// USB-only so it doesn't pay the BLE scan latency.
pub async fn find_all_devices() -> Result<Vec<AttentioDevice>, AttentioError> {
    let mut devices = find_devices().await?;
    devices.extend(find_ble_devices().await);
    Ok(devices)
}

/// Probe dual-CDC devices to determine which port is the protocol port.
///
/// On Linux, `/dev/ttyACM0` is always interface 0 (serial prints) and
/// `/dev/ttyACM1` is interface 1 (protocol), so alphabetical order works.
/// On Windows, COM port numbers are assigned arbitrarily by the OS and don't
/// reliably correspond to USB interface order. This function opens each
/// candidate port, sends a PING command, and swaps CDC0/CDC1 if the currently
/// assigned protocol port doesn't respond.
async fn probe_and_fix_cdc_roles(devices: &mut [AttentioDevice]) {
    for device in devices.iter_mut() {
        // Only probe dual-CDC devices in Normal mode that have both ports.
        if device.mode != DeviceMode::Normal {
            continue;
        }
        let (Some(cdc0_path), Some(cdc1_path)) = (
            device.cdc0.as_ref().map(|p| p.path.clone()),
            device.cdc1.as_ref().map(|p| p.path.clone()),
        ) else {
            continue;
        };

        // If we already know the protocol port from a previous successful
        // probe, apply the cached assignment without re-probing. This avoids
        // Access Denied errors when the ApClient already holds the port.
        if let Some(cached_protocol_port) = cdc_protocol_cache_lookup(&device.serial) {
            if cached_protocol_port != cdc1_path {
                debug!(
                    "Applying cached CDC role swap for {}: CDC1={}",
                    device.serial, cached_protocol_port
                );
                device.cdc0 = Some(CdcPort {
                    path: cdc1_path,
                    role: CdcRole::SerialPrints,
                });
                device.cdc1 = Some(CdcPort {
                    path: cdc0_path,
                    role: CdcRole::Protocol,
                });
            }
            // Cached assignment matches current — no swap needed.
            continue;
        }

        // Try the currently assigned protocol port (CDC1) first.
        debug!("Probing protocol port: {} (CDC1)", cdc1_path);
        if probe_port_ping(&cdc1_path).await {
            debug!("Port {} responded to PING — CDC1 assignment correct", cdc1_path);
            cdc_protocol_cache_remember(&device.serial, &cdc1_path);
            continue;
        }

        // Protocol port didn't respond. Try the serial prints port (CDC0).
        debug!("Port {} did not respond to PING — trying {} as protocol", cdc1_path, cdc0_path);
        if probe_port_ping(&cdc0_path).await {
            debug!(
                "Port {} responded to PING — swapping CDC0/CDC1",
                cdc0_path
            );
            device.cdc0 = Some(CdcPort {
                path: cdc1_path,
                role: CdcRole::SerialPrints,
            });
            let protocol_port = cdc0_path.clone();
            device.cdc1 = Some(CdcPort {
                path: cdc0_path,
                role: CdcRole::Protocol,
            });
            cdc_protocol_cache_remember(&device.serial, &protocol_port);
        } else {
            warn!(
                "Neither {} nor {} responded to PING — keeping default CDC assignment",
                cdc1_path, cdc0_path
            );
        }
    }
}

/// Probe a port by sending an AP PING command.
///
/// Opens the port, brief settle delay, sends PING, waits for a response.
/// Returns `true` if the port responded, `false` on timeout or error.
/// The port is closed when the function returns.
async fn probe_port_ping(port_path: &str) -> bool {
    // Brief delay to let CDC ACM link settle after enumeration.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut client = match ApClient::open(port_path) {
        Ok(c) => c,
        Err(e) => {
            debug!("Probe: failed to open {}: {}", port_path, e);
            return false;
        }
    };

    // Drain stale bytes from the receive buffer.
    client.drain().await;

    // Try PING with a short timeout.
    let result = tokio::time::timeout(std::time::Duration::from_millis(500), client.ping()).await;

    // ApClient is dropped here, closing the port.
    drop(client);

    match result {
        Ok(Ok(())) => {
            debug!("Probe: PING response received on {}", port_path);
            true
        }
        Ok(Err(e)) => {
            debug!("Probe: PING error on {}: {}", port_path, e);
            false
        }
        Err(_) => {
            debug!("Probe: PING timed out on {}", port_path);
            false
        }
    }
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

        // Check if this is our device (pid.codes VID/PID or STM32 DFU fallback)
        if !config::is_known_device(desc.vendor_id(), desc.product_id()) {
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
        // Also treat STM32 DFU fallback devices (blank boards) as bootloader
        let is_stm_dfu = config::is_stm_dfu_device(desc.vendor_id(), desc.product_id());
        if mode == DeviceMode::Bootloader || (is_stm_dfu && mode == DeviceMode::Unknown) {
            let mode = DeviceMode::Bootloader;
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
                transport: Transport::Usb,
                ble_address: None,
                paired: None,
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

        if !config::is_known_device(desc.vendor_id(), desc.product_id()) {
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
/// Opens a short-lived connection to the device's AP port, drains any stale
/// bytes in the receive buffer, sends a SETTINGS_GET("device_name") command,
/// and returns the value. Retries once on failure with a backoff delay.
async fn query_device_name(port_path: &str) -> Result<String, AttentioError> {
    // Brief delay to let CDC ACM link settle after enumeration.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    match query_device_name_once(port_path).await {
        Ok(name) => Ok(name),
        Err(first_err) => {
            debug!(
                "First attempt to read device_name from {} failed: {} — retrying",
                port_path, first_err
            );
            // Backoff before retry
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            query_device_name_once(port_path).await
        }
    }
}

/// Single attempt to query `device_name` from a device.
///
/// Opens the port, drains any stale bytes from the receive buffer (leftover
/// debug prints or previous session data), then sends the AP command.
async fn query_device_name_once(port_path: &str) -> Result<String, AttentioError> {
    let mut client = ApClient::open(port_path)?;

    // Drain stale bytes from the receive buffer before sending the command.
    // The firmware may have queued debug prints or leftover data that would
    // confuse the AP parser.
    client.drain().await;

    let (_key, value) = client.settings_get("device_name").await?;
    Ok(value)
}

/// Read the USB serial number for a tty device from sysfs (Linux only).
///
/// The serial number is stored at `/sys/class/tty/<ttyname>/device/../serial`,
/// which points to the parent USB device's serial attribute. This is more
/// reliable than `serialport`'s enumeration, which often returns `None` for
/// serial_number on Linux.
#[cfg(target_os = "linux")]
fn read_serial_from_sysfs(port_path: &str) -> Option<String> {
    // Extract tty name from path (e.g. "/dev/ttyACM0" -> "ttyACM0")
    let tty_name = port_path.rsplit('/').next()?;
    let sysfs_path = format!("/sys/class/tty/{}/device/../serial", tty_name);
    match std::fs::read_to_string(&sysfs_path) {
        Ok(s) => {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        Err(e) => {
            debug!(
                "Failed to read serial from sysfs for {}: {}",
                tty_name, e
            );
            None
        }
    }
}

/// Resolve the USB serial number for a port, using platform-specific methods.
///
/// On Linux, `serialport` often returns `serial_number: None`, so we read
/// the serial from sysfs as a fallback. On other platforms, we rely on
/// `serialport`'s value (which is typically correct on Windows and macOS).
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
fn resolve_port_serial(port_path: &str, serialport_serial: Option<&str>) -> String {
    // If serialport already gave us a serial, use it
    if let Some(s) = serialport_serial {
        if !s.is_empty() {
            return s.to_string();
        }
    }

    // On Linux, try sysfs as fallback
    #[cfg(target_os = "linux")]
    if let Some(s) = read_serial_from_sysfs(port_path) {
        debug!(
            "Resolved serial from sysfs for {}: {}",
            port_path, s
        );
        return s;
    }

    "unknown".to_string()
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
                if config::is_known_device(usb_info.vid, usb_info.pid) =>
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

    // Group ports by serial number, resolving via sysfs on Linux when
    // serialport returns None (which is common on Linux).
    let mut by_serial: HashMap<String, Vec<RawUsbPort>> = HashMap::new();
    for port in attentio_ports {
        let serial = resolve_port_serial(
            &port.path,
            port.info.serial_number.as_deref(),
        );
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
                transport: Transport::Usb,
                ble_address: None,
                paired: None,
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
                transport: Transport::Usb,
                ble_address: None,
                paired: None,
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
            _ => prompt_device_selection(devices),
        },
    }
}

/// Prompt the user to select a device when multiple are connected.
///
/// If stdin is a TTY, displays an interactive numbered list and reads the
/// user's choice. Otherwise, falls back to the `MultipleDevices` error so
/// scripts and piped invocations don't hang.
fn prompt_device_selection(
    devices: Vec<AttentioDevice>,
) -> Result<AttentioDevice, AttentioError> {
    let serials = devices
        .iter()
        .map(|d| d.serial.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    // Non-interactive context — keep the existing error behaviour.
    if !std::io::stdin().is_terminal() {
        return Err(AttentioError::MultipleDevices { serials });
    }

    let mut stderr = std::io::stderr();

    // Print the device list to stderr so it doesn't pollute stdout (e.g. --json).
    writeln!(stderr, "\nMultiple devices found. Select a device:\n").ok();
    for (i, dev) in devices.iter().enumerate() {
        let label = match &dev.product {
            Some(name) => format!("{} ({})", dev.serial, name),
            None => dev.serial.clone(),
        };
        writeln!(stderr, "  [{}] {}", i + 1, label).ok();
    }
    writeln!(stderr, "  [0] Cancel").ok();
    writeln!(stderr).ok();

    // Read choice — loop until we get a valid input.
    let stdin = std::io::stdin();
    loop {
        write!(stderr, "Enter choice [0-{}]: ", devices.len()).ok();
        stderr.flush().ok();

        let mut input = String::new();
        if stdin.lock().read_line(&mut input).is_err() || input.is_empty() {
            return Err(AttentioError::MultipleDevices { serials });
        }

        match input.trim().parse::<usize>() {
            Ok(0) => return Err(AttentioError::MultipleDevices { serials }),
            Ok(n) if n <= devices.len() => {
                return Ok(devices.into_iter().nth(n - 1).unwrap());
            }
            _ => {
                writeln!(stderr, "Invalid choice. Try again.").ok();
            }
        }
    }
}
