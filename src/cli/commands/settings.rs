use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::SettingsAction;
use crate::json_output;
use crate::protocol::{open_client, ApClient};

/// Execute the `settings` command — manage device settings via AP protocol.
///
/// Defaults to `list` when no subcommand is specified.
pub async fn execute(
    action: Option<&SettingsAction>,
    device: Option<&str>,
    json: bool,
) -> Result<()> {
    let dev = crate::device::discovery::resolve_device(device)
        .await
        .context("failed to resolve device")?;

    let mut client = open_client(Some(&dev.serial)).await?;

    match action {
        None | Some(SettingsAction::List) => execute_list(&mut client, &dev.serial, json).await,
        Some(SettingsAction::Get { key }) => execute_get(&mut client, key, &dev.serial, json).await,
        Some(SettingsAction::Set { key, value }) => {
            execute_set(&mut client, key, value, &dev.serial, json).await
        }
        Some(SettingsAction::Save { file }) => {
            execute_save(&mut client, file, &dev.serial, json).await
        }
        Some(SettingsAction::Load { file }) => {
            execute_load(&mut client, file, &dev.serial, json).await
        }
    }
}

/// `settings list` — list all settings with their current values.
async fn execute_list(client: &mut ApClient, serial: &str, json: bool) -> Result<()> {
    let entries = client
        .settings_list()
        .await
        .context("failed to list settings")?;

    if json {
        let mut data = serde_json::Map::new();
        data.insert("device".to_string(), json!(serial));
        let settings: serde_json::Map<String, serde_json::Value> =
            entries.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        data.insert("settings".to_string(), serde_json::Value::Object(settings));
        println!(
            "{}",
            json_output::format_success(serde_json::Value::Object(data))
        );
    } else {
        let max_key_len = entries.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (key, value) in &entries {
            println!("  {:<width$}  {}", key, value, width = max_key_len);
        }
    }

    Ok(())
}

/// `settings get <key>` — get the value of a single setting.
async fn execute_get(client: &mut ApClient, key: &str, serial: &str, json: bool) -> Result<()> {
    let (_key, value) = client
        .settings_get(key)
        .await
        .context(format!("failed to get setting '{}'", key))?;

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

/// `settings set <key> <value>` — set the value of a setting.
async fn execute_set(
    client: &mut ApClient,
    key: &str,
    value: &str,
    serial: &str,
    json: bool,
) -> Result<()> {
    client
        .settings_set(key, value)
        .await
        .context(format!("failed to set setting '{}' = '{}'", key, value))?;

    if json {
        let output = json!({
            "device": serial,
            "key": key,
            "value": value,
            "message": format!("Setting '{}' updated", key),
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Setting '{}' updated to '{}'.", key, value);
    }

    Ok(())
}

/// `settings save <file>` — save all settings to a JSON file.
async fn execute_save(client: &mut ApClient, file: &str, serial: &str, json: bool) -> Result<()> {
    let entries = client
        .settings_list()
        .await
        .context("failed to list settings for save")?;

    // Use BTreeMap for deterministic key ordering in output.
    let settings: BTreeMap<String, String> = entries.into_iter().collect();

    let json_str =
        serde_json::to_string_pretty(&settings).context("failed to serialize settings")?;

    std::fs::write(file, &json_str).context(format!("failed to write settings to '{}'", file))?;

    if json {
        let output = json!({
            "device": serial,
            "file": file,
            "settings_count": settings.len(),
            "message": format!("Settings saved to '{}'", file),
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Saved {} setting(s) to '{}'.", settings.len(), file);
    }

    Ok(())
}

/// `settings load <file>` — load settings from a JSON file and apply them.
async fn execute_load(client: &mut ApClient, file: &str, serial: &str, json: bool) -> Result<()> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!("settings file not found: {}", file);
    }

    let content = std::fs::read_to_string(path)
        .context(format!("failed to read settings from '{}'", file))?;

    let settings: BTreeMap<String, String> = serde_json::from_str(&content)
        .context(format!("failed to parse settings file '{}'", file))?;

    if settings.is_empty() {
        if json {
            let output = json!({
                "device": serial,
                "file": file,
                "settings_count": 0,
                "message": "No settings to apply (file is empty)",
            });
            println!("{}", json_output::format_success(output));
        } else {
            println!("No settings to apply (file is empty).");
        }
        return Ok(());
    }

    let mut applied = 0;
    for (key, value) in &settings {
        client
            .settings_set(key, value)
            .await
            .context(format!("failed to set '{}' = '{}'", key, value))?;

        if !json {
            println!("  {} = {}", key, value);
        }
        applied += 1;
    }

    if json {
        let output = json!({
            "device": serial,
            "file": file,
            "settings_count": applied,
            "message": format!("Applied {} setting(s) from '{}'", applied, file),
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Applied {} setting(s) from '{}'.", applied, file);
    }

    Ok(())
}
