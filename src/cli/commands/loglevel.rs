use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::LoglevelAction;
use crate::json_output;
use crate::protocol::open_client;

/// Log level names indexed by level value (0-4).
pub const LEVEL_NAMES: [&str; 5] = ["NONE", "ERROR", "WARN", "INFO", "DEBUG"];

/// Get the name for a log level value.
pub fn level_name(level: u8) -> &'static str {
    LEVEL_NAMES.get(level as usize).unwrap_or(&"UNKNOWN")
}

pub async fn execute(action: &LoglevelAction, device: Option<&str>, json: bool) -> Result<()> {
    let mut client = open_client(device).await?;

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
