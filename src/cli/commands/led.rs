use anyhow::Result;
use serde_json::json;

use crate::json_output;

/// Execute the `led` command — control LED mode and settings.
pub async fn execute(
    mode: &str,
    options: &[String],
    device: Option<&str>,
    json: bool,
) -> Result<()> {
    let _ = (mode, options, device);
    let err = anyhow::anyhow!("'led' command is not yet implemented (Phase 4)");

    if json {
        println!("{}", json_output::format_error(&err, json!({})));
    } else {
        eprintln!("Error: {:#}", err);
    }

    Err(err)
}
