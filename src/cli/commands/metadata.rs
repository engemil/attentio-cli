use anyhow::{Context, Result};
use serde_json::json;
use tracing::info;

use crate::cli::MetadataAction;
use crate::device::connection::DeviceConnection;
use crate::device::discovery::resolve_device;
use crate::json_output;

/// Execute the `metadata` command — read device metadata (read-only).
pub async fn execute(
    action: &Option<MetadataAction>,
    device: Option<&str>,
    json: bool,
) -> Result<()> {
    match action {
        // Default to list if no action specified
        None => execute_list(device, json).await,
        Some(MetadataAction::List) => execute_list(device, json).await,
        Some(MetadataAction::Get { key }) => execute_get(key, device, json).await,
    }
}

/// List all metadata from the device.
async fn execute_list(device: Option<&str>, json: bool) -> Result<()> {
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    let response = conn
        .send_command("metadata")
        .await
        .context("failed to list metadata")?;

    // Parse key=value lines
    let lines: Vec<&str> = response.lines().collect();
    let mut entries = Vec::new();
    for line in lines {
        if let Some((key, value)) = line.split_once('=') {
            entries.push((key.to_string(), value.to_string()));
        }
    }

    if json {
        let json_entries: Vec<_> = entries
            .iter()
            .map(|(key, value)| {
                json!({
                    "key": key,
                    "value": value
                })
            })
            .collect();

        let output = json!({
            "metadata": json_entries,
            "count": entries.len()
        });

        println!("{}", json_output::format_success(output));
    } else {
        println!("Metadata:");
        println!("  {:<20} {}", "Key", "Value");
        println!("  {:-<20} {:-<30}", "", "");

        for (key, value) in entries.iter() {
            println!("  {:<20} {}", key, value);
        }

        println!("\nTotal: {} fields", entries.len());
    }

    Ok(())
}

/// Get a specific metadata value.
async fn execute_get(key: &str, device: Option<&str>, json: bool) -> Result<()> {
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    let cmd = format!("metadata get {}", key);
    let response = conn
        .send_command(&cmd)
        .await
        .context(format!("failed to get metadata '{}'", key))?;

    // Parse key=value response
    let value = if let Some((_k, v)) = response.trim().split_once('=') {
        v
    } else {
        response.trim()
    };

    if json {
        let data = json!({
            "key": key,
            "value": value
        });
        println!("{}", json_output::format_success(data));
    } else {
        println!("{}", value);
    }
    Ok(())
}
