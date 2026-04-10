use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::PowerAction;
use crate::device::discovery::resolve_device;
use crate::json_output;
use crate::protocol::ApClient;

/// Execute the `power` command — control device power.
pub async fn execute(action: &PowerAction, device: Option<&str>, json: bool) -> Result<()> {
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;

    let port_path = dev
        .ap_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no protocol port", dev.serial))?
        .to_string();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = ApClient::open(&port_path)
        .context(format!("failed to open protocol port {}", port_path))?;

    match action {
        PowerAction::On => {
            client
                .power_on()
                .await
                .context("failed to power on device")?;

            if json {
                let output = json!({ "message": "Device powered on" });
                println!("{}", json_output::format_success(output));
            } else {
                println!("Device powered on.");
            }
        }
        PowerAction::Off => {
            client
                .power_off()
                .await
                .context("failed to power off device")?;

            if json {
                let output = json!({ "message": "Device powered off" });
                println!("{}", json_output::format_success(output));
            } else {
                println!("Device powered off.");
            }
        }
    }

    Ok(())
}
