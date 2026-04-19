use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::MetadataAction;
use crate::json_output;
use crate::protocol::{open_client, ApClient};

/// Execute the `metadata` command — query device metadata via AP protocol.
///
/// Defaults to `list` when no subcommand is specified.
pub async fn execute(
    action: Option<&MetadataAction>,
    device: Option<&str>,
    json: bool,
) -> Result<()> {
    let dev = crate::device::discovery::resolve_device(device)
        .await
        .context("failed to resolve device")?;

    let mut client = open_client(Some(&dev.serial)).await?;

    match action {
        None | Some(MetadataAction::List) => execute_list(&mut client, &dev.serial, json).await,
        Some(MetadataAction::Get { key }) => execute_get(&mut client, key, &dev.serial, json).await,
    }
}

/// `metadata list` — list all metadata fields.
async fn execute_list(client: &mut ApClient, serial: &str, json: bool) -> Result<()> {
    let entries = client
        .get_metadata()
        .await
        .context("failed to query device metadata")?;

    if json {
        let mut data = serde_json::Map::new();
        data.insert("device".to_string(), json!(serial));
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

/// `metadata get <key>` — get the value of a single metadata field.
async fn execute_get(client: &mut ApClient, key: &str, serial: &str, json: bool) -> Result<()> {
    let (_key, value) = client
        .get_metadata_field(key)
        .await
        .context(format!("failed to get metadata field '{}'", key))?;

    if json {
        let output = json!({
            "device": serial,
            "key": key,
            "value": value,
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("{}", value);
    }

    Ok(())
}
