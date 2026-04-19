use std::time::Duration;

use anyhow::Result;
use serde_json::json;

use crate::device::discovery::{self, find_devices, AttentioDevice};
use crate::error::AttentioError;
use crate::json_output;

/// Maximum time allowed for device enumeration (USB scan + per-device AP queries).
const LIST_TIMEOUT: Duration = Duration::from_secs(5);

/// Execute the `list` command — enumerate and display connected device(s).
pub async fn execute(json: bool) -> Result<()> {
    let find_result = tokio::time::timeout(LIST_TIMEOUT, find_devices())
        .await
        .map_err(|_| AttentioError::Timeout {
            seconds: LIST_TIMEOUT.as_secs(),
        })?;

    let devices = find_result?;

    if json {
        print_json(&devices)?;
    } else {
        if devices.is_empty() {
            println!("No device(s) found.");
        } else {
            print_table(&devices);
        }
    }
    Ok(())
}

fn print_json(devices: &[AttentioDevice]) -> Result<()> {
    // Add 1-based index to each device in JSON output
    let devices_with_index: Vec<serde_json::Value> = devices
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let mut v = serde_json::to_value(d).unwrap_or_default();
            if let Some(obj) = v.as_object_mut() {
                obj.insert("index".to_string(), json!(i + 1));
            }
            v
        })
        .collect();
    let output = json!({
        "data": devices_with_index,
    });
    println!("{}", json_output::format_success(output));
    Ok(())
}

fn print_table(devices: &[AttentioDevice]) {
    // Calculate column widths for line 1: #  DEVICE NAME  DEVICE TYPE  STATUS  SERIAL
    let index_width = devices.len().to_string().len().max(1); // min = len("#")

    let device_name_width = devices
        .iter()
        .map(|d| d.product.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(11)
        .max(11); // min = len("DEVICE NAME")

    let device_type_width = devices
        .iter()
        .map(|d| d.device_type.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(11)
        .max(11); // min = len("DEVICE TYPE")

    let mode_width = devices
        .iter()
        .map(|d| format_status(d).len())
        .max()
        .unwrap_or(6)
        .max(6);

    let serial_width = devices
        .iter()
        .map(|d| d.serial.len())
        .max()
        .unwrap_or(6)
        .max(6);

    // Indentation for line 2: align under DEVICE NAME column
    let indent = " ".repeat(index_width + 2);

    // Print header
    println!(
        "{:<index_width$}  {:<device_name_width$}  {:<device_type_width$}  {:<mode_width$}  {:<serial_width$}",
        "#", "DEVICE NAME", "DEVICE TYPE", "STATUS", "SERIAL",
    );
    println!(
        "{:<index_width$}  {:<device_name_width$}  {:<device_type_width$}  {:<mode_width$}  {:<serial_width$}",
        "-".repeat(index_width),
        "-".repeat(device_name_width),
        "-".repeat(device_type_width),
        "-".repeat(mode_width),
        "-".repeat(serial_width),
    );

    // Print rows (2 lines per device, blank line between devices)
    for (i, device) in devices.iter().enumerate() {
        // Line 1: identity
        println!(
            "{:<index_width$}  {:<device_name_width$}  {:<device_type_width$}  {:<mode_width$}  {:<serial_width$}",
            i + 1,
            device.product.as_deref().unwrap_or("-"),
            device.device_type.as_deref().unwrap_or("-"),
            format_status(device),
            device.serial,
        );

        // Line 2: connection details (indented)
        let ports = format_ports(device);
        let usb_loc = device.usb_location.as_deref().unwrap_or("-");
        println!("{}Ports: {}   USB: {}", indent, ports, usb_loc);

        // Blank line between devices (but not after the last one)
        if i + 1 < devices.len() {
            println!();
        }
    }

    println!();
    println!("{} device(s) found.", devices.len());
}

/// Format the STATUS column for a device.
fn format_status(device: &AttentioDevice) -> String {
    device.mode.to_string()
}

fn format_ports(device: &AttentioDevice) -> String {
    let ports = device.all_ports();
    if ports.is_empty() {
        "-".to_string()
    } else {
        ports
            .iter()
            .map(|p| {
                // For bootloader mode, show [Bootloader/DFU] instead of role
                let label = if device.mode == discovery::DeviceMode::Bootloader {
                    "Bootloader/DFU".to_string()
                } else {
                    p.role.to_string()
                };
                format!("{} [{}]", p.path, label)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}
