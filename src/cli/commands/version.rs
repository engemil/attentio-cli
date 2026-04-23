use anyhow::Result;
use serde_json::json;

use crate::json_output;

/// Execute the `version` command — print CLI version.
pub fn execute(json: bool) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");

    if json {
        let output = json!({
            "version": version,
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("attentio {}", version);
    }

    Ok(())
}
