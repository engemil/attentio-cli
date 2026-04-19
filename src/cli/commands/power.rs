use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::PowerAction;
use crate::json_output;
use crate::protocol::open_client;

/// Execute the `power` command — control device power.
pub async fn execute(action: &PowerAction, device: Option<&str>, json: bool) -> Result<()> {
    let mut client = open_client(device).await?;

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
