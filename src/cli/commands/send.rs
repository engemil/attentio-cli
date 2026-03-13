use anyhow::{Context, Result};
use serde_json::json;
use tracing::info;

use crate::device::connection::DeviceConnection;
use crate::device::discovery::resolve_device;

/// Execute the `send` command — send a one-shot command to the device and print the response.
pub async fn execute(cmd: &[String], device: Option<&str>, json: bool) -> Result<()> {
    // Join multiple arguments into a single command string
    let cmd_str = cmd.join(" ");

    // Resolve which device to talk to
    let dev = resolve_device(device).context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    // Open connection and send the command
    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    let response = conn
        .send_command(&cmd_str)
        .await
        .context(format!("failed to send command '{}'", cmd_str))?;

    // Output the result
    if json {
        let output = json!({
            "device": dev.serial,
            "command": cmd_str,
            "response": response,
            "status": "OK",
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if !response.is_empty() {
            println!("{}", response);
        }
        println!("OK");
    }

    Ok(())
}
