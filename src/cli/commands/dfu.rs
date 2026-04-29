use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::device::config;
use crate::device::connection::DeviceConnection;
use crate::device::discovery::{find_devices, find_devices_fast, resolve_device, DeviceMode};
use crate::json_output;
use crate::protocol::packet::{build_packet, CMD_DFU_ENTER};

// ── Firmware header constants ────────────────────────────────────────────────

/// Magic value in the first 4 bytes of the application header (little-endian).
const FIRMWARE_MAGIC: u32 = 0xDEAD_BEEF;

/// Size of the application header in bytes.
const HEADER_SIZE: usize = 32;

/// Offset to the actual firmware vector table (after header and padding).
const VECTOR_TABLE_OFFSET: usize = 0x100;

/// Base address for the application in flash (after bootloader).
const APP_BASE_ADDRESS: u32 = 0x0800_4000;

/// DFU interface number (alt setting 0).
const DFU_IFACE: u8 = 0;

/// DFU alt setting.
const DFU_ALT: u8 = 0;

// ── Timing constants ─────────────────────────────────────────────────────────

/// How long to wait for the device to re-enumerate after entering DFU mode.
const DFU_ENTER_TIMEOUT: Duration = Duration::from_secs(10);

/// How often to poll for DFU device re-enumeration.
const DFU_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Brief delay after sending the DFU shell command before polling.
/// Gives the device time to reboot and the USB stack time to notice.
const POST_REBOOT_DELAY: Duration = Duration::from_secs(1);

/// How long to wait for the device to re-enumerate in Normal mode after flashing.
const POST_FLASH_TIMEOUT: Duration = Duration::from_secs(10);

// ── Firmware header validation ───────────────────────────────────────────────

/// Parsed firmware application header (first 32 bytes of the binary).
#[derive(Debug)]
struct FirmwareHeader {
    magic: u32,
    version: u32,
    size: u32,
    crc32: u32,
    vid: u16,
    pid: u16,
}

impl FirmwareHeader {
    /// Parse the 32-byte application header from the start of the firmware binary.
    fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            anyhow::bail!(
                "firmware file too small ({} bytes) — expected at least {} byte header",
                data.len(),
                HEADER_SIZE
            );
        }

        Ok(Self {
            magic: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            version: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            size: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            crc32: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
            vid: u16::from_le_bytes([data[16], data[17]]),
            pid: u16::from_le_bytes([data[18], data[19]]),
        })
    }

    /// Validate the header fields. Returns Ok on success, Err with description on failure.
    fn validate(&self, data: &[u8]) -> Result<()> {
        // Magic must match exactly
        if self.magic != FIRMWARE_MAGIC {
            anyhow::bail!(
                "invalid firmware header: magic 0x{:08X} does not match expected 0x{:08X} — \
                 is this a signed firmware binary?",
                self.magic,
                FIRMWARE_MAGIC
            );
        }

        // VID check: warn if not our registered VID (could be wrong firmware)
        if self.vid != config::ATTENTIO_VID {
            warn!(
                "firmware VID 0x{:04X} does not match expected 0x{:04X} (pid.codes)",
                self.vid, config::ATTENTIO_VID
            );
        }

        // Size sanity check — header.size should roughly correspond to file size
        let file_size = data.len();
        let payload_size = file_size.saturating_sub(VECTOR_TABLE_OFFSET);
        if self.size == 0 {
            warn!("firmware header reports size = 0 (unsigned binary?)");
        } else if (self.size as usize) > payload_size + 1024 {
            warn!(
                "firmware header size ({}) is larger than payload ({} bytes) — possible mismatch",
                self.size, payload_size
            );
        }

        // CRC32 check over the payload (after vector table offset)
        if self.crc32 != 0 && data.len() > VECTOR_TABLE_OFFSET {
            let computed_crc = crc32fast::hash(&data[VECTOR_TABLE_OFFSET..]);
            if computed_crc != self.crc32 {
                anyhow::bail!(
                    "firmware CRC32 mismatch: header says 0x{:08X}, computed 0x{:08X} — \
                     file may be corrupted",
                    self.crc32,
                    computed_crc
                );
            }
            debug!("firmware CRC32 verified: 0x{:08X}", self.crc32);
        }

        debug!(
            "firmware header: magic=0x{:08X} version={} size={} crc32=0x{:08X} vid=0x{:04X} pid=0x{:04X}",
            self.magic, self.version, self.size, self.crc32, self.vid, self.pid
        );

        Ok(())
    }
}

// ── DFU-enter: reboot device into bootloader ─────────────────────────────────

/// Execute the `dfu-enter` command — reboot device into DFU bootloader.
pub async fn execute_enter(device: Option<&str>, json: bool) -> Result<()> {
    let serial = execute_enter_internal(device).await?;

    if json {
        let output = json!({
            "device": serial,
            "action": "dfu-enter",
            "message": "Device entered bootloader (DFU) mode",
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Device '{}' entered bootloader (DFU) mode.", serial);
    }
    Ok(())
}

/// Internal: send the AP DFU_ENTER command to reboot the device into bootloader.
///
/// The firmware no longer has a ChibiOS shell — DFU enter is handled via
/// the Attentio Protocol (AP) interface on CDC1 (ap_port).
/// We send a raw 4-byte AP packet: [SYNC=0xA5, LEN=0x01, CMD=0x70, CRC8=0x42].
/// The device writes 0xDEADBEEF to RAM and triggers NVIC_SystemReset()
/// immediately — the USB connection will drop with no response.
///
/// Returns the device serial number on success.
async fn execute_enter_internal(device: Option<&str>) -> Result<String> {
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;

    if dev.mode == DeviceMode::Bootloader {
        anyhow::bail!(
            "device '{}' is already in bootloader mode — no need to enter DFU",
            dev.serial
        );
    }

    let port_path = dev
        .ap_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no protocol port", dev.serial))?
        .to_string();

    let serial = dev.serial.clone();
    info!("Sending DFU enter command to {} on {}", serial, port_path);

    // Open connection and send the AP DFU_ENTER packet.
    // AP packet format: [SYNC 0xA5] [LEN] [CMD] [CRC8]
    // DFU_ENTER (0x70) has no payload, so LEN=1.
    let ap_dfu_enter_packet = build_packet(CMD_DFU_ENTER, &[]);

    let mut conn = DeviceConnection::open(&port_path)
        .with_context(|| format!("failed to open serial port {}", port_path))?;

    // Brief delay so the CDC ACM link is up.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send the raw AP binary packet — the device will reboot immediately.
    match conn.write_raw(&ap_dfu_enter_packet).await {
        Ok(_) => {
            debug!("AP DFU_ENTER packet sent successfully");
        }
        Err(e) => {
            // The device may reboot so fast that the write fails — that's OK.
            let err_str = format!("{}", e);
            if err_str.contains("Broken pipe") || err_str.contains("connection") {
                debug!("device rebooted during write (expected): {}", e);
            } else {
                return Err(anyhow::anyhow!(
                    "unexpected error sending DFU enter command: {}",
                    e
                ));
            }
        }
    }

    // Drop the connection explicitly before polling.
    drop(conn);

    // Wait for the device to re-enumerate in bootloader mode.
    eprintln!("Waiting for device to enter bootloader (DFU) mode...");
    tokio::time::sleep(POST_REBOOT_DELAY).await;

    wait_for_device_mode(
        DeviceMode::Bootloader,
        Some(&serial),
        DFU_ENTER_TIMEOUT,
        true,
    )
    .await?;

    Ok(serial)
}

/// Poll until a device with the specified mode appears on USB, or timeout.
///
/// If `serial` is provided, waits for a device with that serial.
/// Otherwise waits for any device in the target mode.
///
/// Uses lightweight enumeration (no AP queries) since we only need to
/// detect device presence and mode.
async fn wait_for_device_mode(
    target_mode: DeviceMode,
    serial: Option<&str>,
    timeout: Duration,
    hard_error: bool,
) -> Result<()> {
    let start = Instant::now();
    let mode_label = match target_mode {
        DeviceMode::Bootloader => "bootloader",
        DeviceMode::Normal => "normal",
        DeviceMode::Unknown => "unknown",
    };

    loop {
        let devices = find_devices_fast().unwrap_or_default();
        let found = devices
            .iter()
            .any(|d| d.mode == target_mode && serial.is_none_or(|s| d.serial == s));

        if found {
            debug!(
                "Device detected in {} mode (serial: {})",
                mode_label,
                serial.unwrap_or("any")
            );
            return Ok(());
        }

        if start.elapsed() > timeout {
            if hard_error {
                anyhow::bail!(
                    "timed out waiting for device to enter {} mode ({:.0}s) — \
                     device may not have rebooted correctly",
                    mode_label,
                    timeout.as_secs_f64()
                );
            } else {
                warn!(
                    "device did not re-enumerate in {} mode within {:.0}s",
                    mode_label,
                    timeout.as_secs_f64()
                );
                return Ok(());
            }
        }

        tokio::time::sleep(DFU_POLL_INTERVAL).await;
    }
}

// ── DFU flash: validate + flash firmware ─────────────────────────────────────

/// Execute the `dfu` command — flash firmware via DFU.
pub async fn execute(firmware: &str, device: Option<&str>, json: bool) -> Result<()> {
    execute_flash_internal(firmware, device).await?;

    if json {
        let output = json!({
            "action": "dfu",
            "firmware": firmware,
            "message": "Firmware flashed successfully",
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Firmware flashed successfully.");
    }
    Ok(())
}

/// Internal: validate firmware binary, ensure device is in bootloader mode, flash.
async fn execute_flash_internal(firmware_path: &str, device: Option<&str>) -> Result<()> {
    // ── Step 1: Read and validate firmware binary ────────────────────────────

    let path = Path::new(firmware_path);
    if !path.exists() {
        anyhow::bail!("firmware file not found: {}", firmware_path);
    }

    let firmware_data = std::fs::read(path)
        .with_context(|| format!("failed to read firmware file: {}", firmware_path))?;

    if firmware_data.is_empty() {
        anyhow::bail!("firmware file is empty: {}", firmware_path);
    }

    eprintln!(
        "Firmware: {} ({} bytes)",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| firmware_path.to_string()),
        firmware_data.len()
    );

    // Validate the application header
    let header = FirmwareHeader::parse(&firmware_data)?;
    header.validate(&firmware_data)?;

    // ── Step 2: Ensure device is in bootloader mode ─────────────────────────

    let devices = find_devices().await.unwrap_or_default();

    // Determine the target device serial for filtering.
    // If --device was given, use it. Otherwise try to find the serial from
    // available devices (bootloader or normal).
    let target_serial: Option<String> = if let Some(s) = device {
        Some(s.to_string())
    } else {
        // If there's exactly one device total, use its serial
        if devices.len() == 1 && devices[0].serial != "unknown" {
            Some(devices[0].serial.clone())
        } else {
            None
        }
    };

    let has_bootloader = devices.iter().any(|d| {
        d.mode == DeviceMode::Bootloader && target_serial.as_deref().is_none_or(|s| d.serial == s)
    });

    if !has_bootloader {
        // Try to find a normal-mode device and auto-enter DFU
        let normal_devices: Vec<_> = devices
            .iter()
            .filter(|d| d.mode == DeviceMode::Normal)
            .collect();

        if normal_devices.is_empty() {
            anyhow::bail!(
                "no Attentio device found — connect a device and try again\n\
                 Hint: if the device is in bootloader mode but not detected, \
                 check USB permissions (udev rules)"
            );
        }

        eprintln!("Device is in normal mode — entering DFU bootloader automatically...");
        execute_enter_internal(device).await?;
        eprintln!("Device is now in bootloader mode.");
    } else {
        eprintln!("Device detected in bootloader mode.");
    }

    // ── Step 3: Flash firmware via DFU ──────────────────────────────────────

    let firmware_len = firmware_data.len();
    let serial_for_flash = target_serial.clone();

    // DfuSync is !Send, so we must run it on the current thread via spawn_blocking
    // with a dedicated rusb context. We move the firmware data into the closure.
    let flash_result = tokio::task::spawn_blocking(move || {
        flash_dfu_device(&firmware_data, firmware_len, serial_for_flash.as_deref())
    })
    .await
    .context("DFU flash task panicked")?;

    flash_result?;

    // ── Step 4: Wait for device to come back in normal mode ─────────────────

    eprintln!("Waiting for device to reboot...");
    tokio::time::sleep(POST_REBOOT_DELAY).await;
    wait_for_device_mode(
        DeviceMode::Normal,
        target_serial.as_deref(),
        POST_FLASH_TIMEOUT,
        false,
    )
    .await?;

    Ok(())
}

/// Open a DFU device by USB serial number.
///
/// Manually enumerates USB devices, finds the one with matching VID/PID/serial,
/// and constructs a `DfuLibusb` instance via `from_usb_device`.
fn open_dfu_by_serial<C: rusb::UsbContext>(
    context: &C,
    target_serial: &str,
) -> Result<dfu_libusb::Dfu<C>> {
    if let Some((device, _desc, handle)) =
        find_matching_attentio_usb_device(context, Some(target_serial))?
    {
        debug!("Found DFU device with serial {}", target_serial);
        return dfu_libusb::DfuLibusb::from_usb_device(device, handle, DFU_IFACE, DFU_ALT)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to open DFU device (serial {}): {} — \
                     check USB permissions (udev rules) and ensure the device is in bootloader mode",
                    target_serial,
                    e
                )
            });
    }

    anyhow::bail!(
        "DFU device with serial '{}' not found — \
         ensure the device is connected and in bootloader mode",
        target_serial
    )
}

/// Synchronous DFU flash using dfu-libusb.
///
/// This runs on a blocking thread (via `spawn_blocking`) because `DfuSync` is `!Send`.
/// If `serial` is provided, opens the specific device with that USB serial number.
fn flash_dfu_device(firmware_data: &[u8], firmware_len: usize, serial: Option<&str>) -> Result<()> {
    // Try to flash; if it fails due to invalid state, reset USB and retry once.
    match flash_dfu_device_inner(firmware_data, firmware_len, serial) {
        Ok(()) => Ok(()),
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("invalid state") {
                // Device is stuck in a bad state (e.g., DfuDnloadIdle from a previous
                // incomplete transfer). Reset USB and retry.
                eprintln!("Device in invalid state, resetting USB and retrying...");
                reset_dfu_device(serial)?;
                flash_dfu_device_inner(firmware_data, firmware_len, serial)
            } else {
                Err(e)
            }
        }
    }
}

/// Reset the DFU device via USB reset to clear stale state.
///
/// If `serial` is provided, resets only the device with that USB serial number.
fn reset_dfu_device(serial: Option<&str>) -> Result<()> {
    let context = rusb::Context::new().context("failed to create USB context")?;

    if let Some((_device, _desc, handle)) = find_matching_attentio_usb_device(&context, serial)? {
        handle.reset().context("failed to reset DFU device")?;
        debug!("DFU device reset successfully");

        // Wait for device to re-enumerate after reset
        drop(handle);
        wait_for_dfu_device_sync(serial)?;
        return Ok(());
    }

    anyhow::bail!("DFU device not found for reset")
}

/// Synchronous poll until a DFU device appears on USB, or timeout.
///
/// If `serial` is provided, waits for a device with that USB serial number.
fn wait_for_dfu_device_sync(serial: Option<&str>) -> Result<()> {
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    let poll_interval = Duration::from_millis(200);

    loop {
        std::thread::sleep(poll_interval);

        let context = rusb::Context::new().context("failed to create USB context")?;
        if find_matching_attentio_usb_device(&context, serial)?.is_some() {
            debug!("DFU device re-enumerated after reset");
            return Ok(());
        }

        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for DFU device to re-enumerate after reset");
        }
    }
}

fn find_matching_attentio_usb_device<C: rusb::UsbContext>(
    context: &C,
    serial: Option<&str>,
) -> Result<
    Option<(
        rusb::Device<C>,
        rusb::DeviceDescriptor,
        rusb::DeviceHandle<C>,
    )>,
> {
    let devices = context
        .devices()
        .context("failed to enumerate USB devices")?;

    for device in devices.iter() {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if !config::is_known_device(desc.vendor_id(), desc.product_id()) {
            continue;
        }

        let handle = match device.open() {
            Ok(h) => h,
            Err(_) => continue,
        };

        if let Some(target_serial) = serial {
            match handle.read_serial_number_string_ascii(&desc) {
                Ok(s) if s == target_serial => {
                    return Ok(Some((device, desc, handle)));
                }
                _ => continue,
            }
        } else {
            return Ok(Some((device, desc, handle)));
        }
    }

    Ok(None)
}

/// Inner flash implementation — called by flash_dfu_device with retry logic.
///
/// If `serial` is provided, opens the specific DFU device with that USB serial.
/// Otherwise opens the first device matching VID/PID.
fn flash_dfu_device_inner(
    firmware_data: &[u8],
    firmware_len: usize,
    serial: Option<&str>,
) -> Result<()> {
    let context = rusb::Context::new().context("failed to create USB context")?;

    // Open the DFU device — if serial is specified, manually enumerate and
    // filter by serial using from_usb_device(); otherwise use open() for
    // backward compatibility.
    let mut dfu = if let Some(target_serial) = serial {
        open_dfu_by_serial(&context, target_serial)?
    } else {
        // Try pid.codes VID/PID first, then STM32 DFU fallback
        dfu_libusb::DfuLibusb::open(
            &context,
            config::ATTENTIO_VID,
            config::ATTENTIO_PID,
            DFU_IFACE,
            DFU_ALT,
        )
        .or_else(|_| {
            dfu_libusb::DfuLibusb::open(
                &context,
                config::STM_DFU_VID,
                config::STM_DFU_PID,
                DFU_IFACE,
                DFU_ALT,
            )
        })
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to open DFU device: {} — \
                 check USB permissions (udev rules) and ensure the device is in bootloader mode",
                e
            )
        })?
    };

    // Set the target flash address (after bootloader)
    dfu.override_address(APP_BASE_ADDRESS);

    // Flush stderr to ensure previous messages are visible before progress output.
    let _ = std::io::stderr().flush();

    // Spinner style with braille dots — last entry is the "frozen" final frame
    let spinner_style = ProgressStyle::with_template("{spinner:.cyan} {msg}")
        .unwrap()
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠿"]);

    // Start with erase spinner
    let erase_bar = ProgressBar::new_spinner();
    erase_bar.set_style(spinner_style.clone());
    erase_bar.set_message("Erasing application flash...");
    erase_bar.enable_steady_tick(Duration::from_millis(80));

    // Track state across progress callbacks
    let mut erase_finished = false;
    let mut total_written: usize = 0;
    let mut write_bar: Option<ProgressBar> = None;

    dfu.with_progress(move |bytes_written| {
        if !erase_finished {
            // First callback = erase phase done, switch to write phase
            erase_finished = true;
            erase_bar.set_style(
                ProgressStyle::with_template("{spinner:.cyan} {msg}")
                    .unwrap()
                    .tick_strings(&["⠿"]), // frozen final frame
            );
            erase_bar.finish_with_message("Erasing application flash... done");

            // Create write progress bar
            let bar = ProgressBar::new(firmware_len as u64);
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.cyan} Flashing application firmware... ({bytes}/{total_bytes}, {percent}%)",
                )
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠿"]),
            );
            bar.enable_steady_tick(Duration::from_millis(80));
            write_bar = Some(bar);
        }

        total_written += bytes_written;
        if let Some(ref bar) = write_bar {
            bar.set_position(total_written as u64);

            if total_written >= firmware_len {
                bar.set_style(
                    ProgressStyle::with_template("{spinner:.cyan} {msg}")
                        .unwrap()
                        .tick_strings(&["⠿"]),
                );
                bar.finish_with_message("Flashing application firmware... done");
            }
        }
    });

    // Flash the firmware
    match dfu.download_from_slice(firmware_data) {
        Ok(_) => Ok(()),
        Err(e) => {
            let err_str = e.to_string();
            // When manifestation triggers a reboot, the device drops off the bus.
            // Depending on OS/timing, this surfaces as an I/O Error, Broken Pipe, or No such device.
            if err_str.contains("Input/Output Error")
                || err_str.contains("Pipe")
                || err_str.contains("No such device")
                || err_str.contains("Not found")
            {
                debug!("DFU download completed with expected USB drop: {}", e);
                Ok(())
            } else {
                Err(anyhow::anyhow!("DFU download failed: {}", e))
            }
        }
    }
}
