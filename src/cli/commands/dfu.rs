use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rusb::UsbContext;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::device::config::{ATTENTIO_PID, ATTENTIO_VID};
use crate::device::connection::DeviceConnection;
use crate::device::discovery::{find_devices, resolve_device, DeviceMode};
use crate::json_output;

// ── Firmware header constants ────────────────────────────────────────────────

/// Magic value in the first 4 bytes of the application header (little-endian).
const FIRMWARE_MAGIC: u32 = 0xDEAD_BEEF;

/// Size of the application header in bytes.
const HEADER_SIZE: usize = 32;

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
    fn validate(&self, file_size: usize) -> Result<()> {
        // Magic must match exactly
        if self.magic != FIRMWARE_MAGIC {
            anyhow::bail!(
                "invalid firmware header: magic 0x{:08X} does not match expected 0x{:08X} — \
                 is this a signed firmware binary?",
                self.magic,
                FIRMWARE_MAGIC
            );
        }

        // VID/PID mismatch is a warning, not a hard error (could be a different product variant)
        if self.vid != ATTENTIO_VID {
            warn!(
                "firmware VID 0x{:04X} does not match expected 0x{:04X}",
                self.vid, ATTENTIO_VID
            );
        }
        if self.pid != ATTENTIO_PID {
            warn!(
                "firmware PID 0x{:04X} does not match expected 0x{:04X}",
                self.pid, ATTENTIO_PID
            );
        }

        // Size sanity check — header.size should roughly correspond to file size
        let payload_size = file_size.saturating_sub(HEADER_SIZE);
        if self.size == 0 {
            warn!("firmware header reports size = 0 (unsigned binary?)");
        } else if (self.size as usize) > payload_size + 1024 {
            warn!(
                "firmware header size ({}) is larger than payload ({} bytes) — possible mismatch",
                self.size, payload_size
            );
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
    match execute_enter_internal(device).await {
        Ok(serial) => {
            if json {
                let output = json!({
                    "device": serial,
                    "action": "dfu-enter",
                    "message": "Device entered DFU bootloader mode",
                });
                println!("{}", json_output::format_success(output));
            } else {
                println!("Device '{}' entered DFU bootloader mode.", serial);
            }
            Ok(())
        }
        Err(e) => {
            if json {
                let context = json!({ "action": "dfu-enter" });
                println!("{}", json_output::format_error(&e, context));
            } else {
                eprintln!("Error: {:#}", e);
            }
            Err(e)
        }
    }
}

/// Internal: send the `dfu` shell command to reboot the device into bootloader.
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
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?
        .to_string();

    let serial = dev.serial.clone();
    info!("Sending DFU enter command to {} on {}", serial, port_path);

    // Open connection and send the `dfu` shell command.
    // The firmware writes 0xDEADBEEF to RAM and triggers NVIC_SystemReset()
    // immediately — the USB connection will drop with no response.
    let mut conn = DeviceConnection::open(&port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    conn.sync_shell()
        .await
        .context("failed to sync with device shell")?;

    // Send the `dfu` command — expect the connection to drop.
    match conn.send_command("dfu").await {
        Ok(_) => {
            // Unlikely but fine — device responded before rebooting.
            debug!("device responded to dfu command before rebooting");
        }
        Err(e) => {
            // Expected: connection closes because device reboots immediately.
            let err_str = format!("{}", e);
            if err_str.contains("connection closed") || err_str.contains("timed out") {
                debug!("device rebooted as expected: {}", e);
            } else {
                return Err(anyhow::anyhow!(
                    "unexpected error sending dfu command: {}",
                    e
                ));
            }
        }
    }

    // Drop the connection explicitly before polling.
    drop(conn);

    // Wait for the device to re-enumerate in bootloader mode.
    eprintln!("Waiting for device to enter bootloader mode...");
    tokio::time::sleep(POST_REBOOT_DELAY).await;

    wait_for_dfu_device().await?;

    Ok(serial)
}

/// Poll until a DFU device appears on USB, or timeout.
async fn wait_for_dfu_device() -> Result<()> {
    let start = Instant::now();

    loop {
        let devices = find_devices().await.unwrap_or_default();
        let has_bootloader = devices.iter().any(|d| d.mode == DeviceMode::Bootloader);

        if has_bootloader {
            debug!("DFU device detected in bootloader mode");
            return Ok(());
        }

        if start.elapsed() > DFU_ENTER_TIMEOUT {
            anyhow::bail!(
                "timed out waiting for device to enter bootloader mode ({:.0}s) — \
                 device may not have rebooted correctly",
                DFU_ENTER_TIMEOUT.as_secs_f64()
            );
        }

        tokio::time::sleep(DFU_POLL_INTERVAL).await;
    }
}

/// Poll until a Normal-mode device re-appears, or timeout.
async fn wait_for_normal_device() -> Result<()> {
    let start = Instant::now();

    loop {
        let devices = find_devices().await.unwrap_or_default();
        let has_normal = devices.iter().any(|d| d.mode == DeviceMode::Normal);

        if has_normal {
            debug!("Device re-enumerated in normal mode");
            return Ok(());
        }

        if start.elapsed() > POST_FLASH_TIMEOUT {
            // Not a hard error — the flash may have succeeded but the device
            // may take longer to boot or the user may need to power-cycle.
            warn!(
                "device did not re-enumerate in normal mode within {:.0}s",
                POST_FLASH_TIMEOUT.as_secs_f64()
            );
            return Ok(());
        }

        tokio::time::sleep(DFU_POLL_INTERVAL).await;
    }
}

// ── DFU flash: validate + flash firmware ─────────────────────────────────────

/// Execute the `dfu` command — flash firmware via DFU.
pub async fn execute(firmware: &str, device: Option<&str>, json: bool) -> Result<()> {
    match execute_flash_internal(firmware, device).await {
        Ok(()) => {
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
        Err(e) => {
            if json {
                let context = json!({
                    "action": "dfu",
                    "firmware": firmware,
                });
                println!("{}", json_output::format_error(&e, context));
            } else {
                eprintln!("Error: {:#}", e);
            }
            Err(e)
        }
    }
}

/// Internal: validate firmware binary, ensure device is in bootloader mode, flash.
async fn execute_flash_internal(firmware_path: &str, device: Option<&str>) -> Result<()> {
    // ── Step 1: Read and validate firmware binary ────────────────────────────

    let path = Path::new(firmware_path);
    if !path.exists() {
        anyhow::bail!("firmware file not found: {}", firmware_path);
    }

    let firmware_data =
        std::fs::read(path).context(format!("failed to read firmware file: {}", firmware_path))?;

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
    header.validate(firmware_data.len())?;

    // ── Step 2: Ensure device is in bootloader mode ─────────────────────────

    let devices = find_devices().await.unwrap_or_default();
    let has_bootloader = devices.iter().any(|d| d.mode == DeviceMode::Bootloader);

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

    // DfuSync is !Send, so we must run it on the current thread via spawn_blocking
    // with a dedicated rusb context. We move the firmware data into the closure.
    let flash_result =
        tokio::task::spawn_blocking(move || flash_dfu_device(&firmware_data, firmware_len))
            .await
            .context("DFU flash task panicked")?;

    flash_result?;

    // ── Step 4: Wait for device to come back in normal mode ─────────────────

    eprintln!("Waiting for device to reboot...");
    tokio::time::sleep(POST_REBOOT_DELAY).await;
    wait_for_normal_device().await?;

    Ok(())
}

/// Synchronous DFU flash using dfu-libusb.
///
/// This runs on a blocking thread (via `spawn_blocking`) because `DfuSync` is `!Send`.
fn flash_dfu_device(firmware_data: &[u8], firmware_len: usize) -> Result<()> {
    // Try to flash; if it fails due to invalid state, reset USB and retry once.
    match flash_dfu_device_inner(firmware_data, firmware_len) {
        Ok(()) => Ok(()),
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("invalid state") {
                // Device is stuck in a bad state (e.g., DfuDnloadIdle from a previous
                // incomplete transfer). Reset USB and retry.
                eprintln!("Device in invalid state, resetting USB and retrying...");
                reset_dfu_device()?;
                flash_dfu_device_inner(firmware_data, firmware_len)
            } else {
                Err(e)
            }
        }
    }
}

/// Reset the DFU device via USB reset to clear stale state.
fn reset_dfu_device() -> Result<()> {
    let context = rusb::Context::new().context("failed to create USB context")?;

    for device in context.devices()?.iter() {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if desc.vendor_id() == ATTENTIO_VID && desc.product_id() == ATTENTIO_PID {
            let handle = device
                .open()
                .context("failed to open DFU device for reset")?;
            handle.reset().context("failed to reset DFU device")?;
            debug!("DFU device reset successfully");

            // Wait for device to re-enumerate after reset
            drop(handle);
            wait_for_dfu_device_sync()?;
            return Ok(());
        }
    }

    anyhow::bail!("DFU device not found for reset")
}

/// Synchronous poll until a DFU device appears on USB, or timeout.
fn wait_for_dfu_device_sync() -> Result<()> {
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    let poll_interval = Duration::from_millis(200);

    loop {
        std::thread::sleep(poll_interval);

        let context = rusb::Context::new().context("failed to create USB context")?;
        for device in context.devices()?.iter() {
            let desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };

            if desc.vendor_id() == ATTENTIO_VID && desc.product_id() == ATTENTIO_PID {
                debug!("DFU device re-enumerated after reset");
                return Ok(());
            }
        }

        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for DFU device to re-enumerate after reset");
        }
    }
}

/// Inner flash implementation — called by flash_dfu_device with retry logic.
fn flash_dfu_device_inner(firmware_data: &[u8], firmware_len: usize) -> Result<()> {
    let context = rusb::Context::new().context("failed to create USB context")?;

    // Open the DFU device
    let mut dfu =
        dfu_libusb::DfuLibusb::open(&context, ATTENTIO_VID, ATTENTIO_PID, DFU_IFACE, DFU_ALT)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to open DFU device: {} — \
            check USB permissions (udev rules) and ensure the device is in bootloader mode",
                    e
                )
            })?;

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
    dfu.download_from_slice(firmware_data)
        .map_err(|e| anyhow::anyhow!("DFU download failed: {}", e))?;

    Ok(())
}
