use anyhow::{Context, Result};
use serde_json::json;

use crate::device::discovery::resolve_device;
use crate::json_output;
use crate::protocol::ApClient;

/// Execute the `metadata` command — query device metadata via AP protocol.
pub async fn execute(device: Option<&str>, json: bool) -> Result<()> {
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;

    let port_path = dev
        .ap_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no protocol port", dev.serial))?
        .to_string();

    // Brief delay to let CDC ACM link settle after enumeration.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = ApClient::open(&port_path)
        .context(format!("failed to open protocol port {}", port_path))?;

    let entries = client
        .get_metadata()
        .await
        .context("failed to query device metadata")?;

    if json {
        let mut data = serde_json::Map::new();
        data.insert("device".to_string(), json!(dev.serial));
        for (key, value) in &entries {
            data.insert(key.clone(), json!(value));
        }
        println!(
            "{}",
            json_output::format_success(serde_json::Value::Object(data))
        );
    } else {
        // Find the longest key for alignment.
        let max_key_len = entries.iter().map(|(k, _)| k.len()).max().unwrap_or(0);

        for (key, value) in &entries {
            println!("  {:<width$}  {}", key, value, width = max_key_len);
        }
    }

    Ok(())
}
