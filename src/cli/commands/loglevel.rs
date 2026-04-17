use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::LoglevelAction;
use crate::device::discovery::resolve_device;
use crate::json_output;
use crate::protocol::ApClient;

const LEVEL_NAMES: [&str; 5] = ["NONE", "ERROR", "WARN", "INFO", "DEBUG"];

fn level_name(level: u8) -> &'static str {
    LEVEL_NAMES.get(level as usize).unwrap_or(&"UNKNOWN")
}

pub async fn execute(action: &LoglevelAction, device: Option<&str>, json: bool) -> Result<()> {
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
        LoglevelAction::Get => {
            let level = client
                .log_get_level()
                .await
                .context("failed to get log level")?;
            if json {
                println!(
                    "{}",
                    json_output::format_success(json!({
                        "level": level,
                        "name": level_name(level),
                    }))
                );
            } else {
                println!("Runtime log level: {} ({})", level, level_name(level));
            }
        }
        LoglevelAction::Set { level } => {
            if *level > 4 {
                anyhow::bail!("log level must be 0-4 (0=NONE, 1=ERROR, 2=WARN, 3=INFO, 4=DEBUG)");
            }
            client
                .log_set_level(*level)
                .await
                .context("failed to set log level")?;
            if json {
                println!(
                    "{}",
                    json_output::format_success(json!({
                        "level": level,
                        "name": level_name(*level),
                        "message": "Runtime log level set (ephemeral, lost on reboot)",
                    }))
                );
            } else {
                println!(
                    "Runtime log level set to {} ({}).",
                    level,
                    level_name(*level)
                );
                println!("Note: this is ephemeral and will be lost on reboot.");
                println!(
                    "Use 'attentio settings set default_loglevel {}' to persist.",
                    level
                );
            }
        }
    }

    Ok(())
}
