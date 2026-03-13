use anyhow::Result;
use serde_json::json;

use crate::cli::SettingsAction;
use crate::json_output;

/// Execute the `settings` command — read/write device settings and presets.
pub async fn execute(action: &SettingsAction, device: Option<&str>, json: bool) -> Result<()> {
    let _ = (action, device);
    let err = anyhow::anyhow!("'settings' command is not yet implemented (Phase 5)");

    if json {
        println!("{}", json_output::format_error(&err, json!({})));
    } else {
        eprintln!("Error: {:#}", err);
    }

    Err(err)
}
