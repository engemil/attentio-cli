use anyhow::Result;
use serde_json::json;

use crate::device::discovery::{self, find_devices, AttentioDevice};
use crate::json_output;

/// Execute the `list` command — enumerate and display connected device(s).
pub async fn execute(json: bool) -> Result<()> {
    match find_devices().await {
        Ok(devices) => {
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
        Err(e) => {
            let err = anyhow::Error::from(e);
            if json {
                println!("{}", json_output::format_error(&err, json!({})));
            } else {
                eprintln!("Error: {:#}", err);
            }
            Err(err)
        }
    }
}

fn print_json(devices: &[AttentioDevice]) -> Result<()> {
    let output = json!({
        "data": devices,
    });
    println!("{}", json_output::format_success(output));
    Ok(())
}

fn print_table(devices: &[AttentioDevice]) {
    // Calculate column widths
    let serial_width = devices
        .iter()
        .map(|d| d.serial.len())
        .max()
        .unwrap_or(6)
        .max(6);

    let mode_width = devices
        .iter()
        .map(|d| format_status(d).len())
        .max()
        .unwrap_or(6)
        .max(6);

    let ports_width = devices
        .iter()
        .map(|d| format_ports(d).len())
        .max()
        .unwrap_or(7)
        .max(7);

    let usb_loc_width = devices
        .iter()
        .map(|d| d.usb_location.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(12)
        .max(12); // min = len("USB LOCATION")

    let device_type_width = devices
        .iter()
        .map(|d| d.device_type.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(11)
        .max(11); // min = len("DEVICE TYPE")

    let device_name_width = devices
        .iter()
        .map(|d| d.product.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(11)
        .max(11); // min = len("DEVICE NAME")

    // Print header
    println!(
        "{:<serial_width$}  {:<device_type_width$}  {:<device_name_width$}  {:<mode_width$}  {:<ports_width$}  {:<usb_loc_width$}",
        "SERIAL", "DEVICE TYPE", "DEVICE NAME", "STATUS", "PORT(S)", "USB LOCATION",
    );
    println!(
        "{:<serial_width$}  {:<device_type_width$}  {:<device_name_width$}  {:<mode_width$}  {:<ports_width$}  {:<usb_loc_width$}",
        "-".repeat(serial_width),
        "-".repeat(device_type_width),
        "-".repeat(device_name_width),
        "-".repeat(mode_width),
        "-".repeat(ports_width),
        "-".repeat(usb_loc_width),
    );

    // Print rows
    for device in devices {
        println!(
            "{:<serial_width$}  {:<device_type_width$}  {:<device_name_width$}  {:<mode_width$}  {:<ports_width$}  {:<usb_loc_width$}",
            device.serial,
            device.device_type.as_deref().unwrap_or("-"),
            device.product.as_deref().unwrap_or("-"),
            format_status(device),
            format_ports(device),
            device.usb_location.as_deref().unwrap_or("-"),
        );
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
