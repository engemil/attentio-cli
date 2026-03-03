use anyhow::Result;

use crate::device::discovery::{find_devices, AttentioDevice};

/// Execute the `list` command — enumerate and display connected device(s).
pub fn execute(json: bool) -> Result<()> {
    let devices = find_devices()?;

    if devices.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No device(s) found.");
        }
        return Ok(());
    }

    if json {
        print_json(&devices)?;
    } else {
        print_table(&devices);
    }

    Ok(())
}

fn print_json(devices: &[AttentioDevice]) -> Result<()> {
    let output = serde_json::to_string_pretty(devices)?;
    println!("{}", output);
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

    let ports_width = devices
        .iter()
        .map(|d| format_ports(d).len())
        .max()
        .unwrap_or(5)
        .max(5);

    let type_width = devices
        .iter()
        .map(|d| format_type(d).len())
        .max()
        .unwrap_or(4)
        .max(4);

    let product_width = devices
        .iter()
        .map(|d| d.product.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(7)
        .max(7);

    // Print header
    println!(
        "{:<serial_width$}  {:<ports_width$}  {:<type_width$}  {:<product_width$}",
        "SERIAL", "PORT(S)", "TYPE", "PRODUCT",
    );

    println!(
        "{:<serial_width$}  {:<ports_width$}  {:<type_width$}  {:<product_width$}",
        "-".repeat(serial_width),
        "-".repeat(ports_width),
        "-".repeat(type_width),
        "-".repeat(product_width),
    );

    // Print rows
    for device in devices {
        println!(
            "{:<serial_width$}  {:<ports_width$}  {:<type_width$}  {:<product_width$}",
            device.serial,
            format_ports(device),
            format_type(device),
            device.product.as_deref().unwrap_or("-"),
        );
    }

    println!();
    println!("{} device(s) found.", devices.len());
}

fn format_ports(device: &AttentioDevice) -> String {
    let ports = device.all_ports();
    if ports.is_empty() {
        "-".to_string()
    } else {
        ports
            .iter()
            .map(|p| format!("{} [{}]", p.path, p.role))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_type(device: &AttentioDevice) -> String {
    if device.cdc0.is_some() && device.cdc1.is_some() {
        "dual CDC".to_string()
    } else {
        "single CDC".to_string()
    }
}
