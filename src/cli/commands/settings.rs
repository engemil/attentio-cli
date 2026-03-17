use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use tracing::info;

use crate::cli::SettingsAction;
use crate::device::connection::DeviceConnection;
use crate::device::discovery::resolve_device;
use crate::json_output;

/// Execute the `settings` command — read/write device settings and presets.
pub async fn execute(
    action: &Option<SettingsAction>,
    device: Option<&str>,
    json: bool,
) -> Result<()> {
    match action {
        // Default to list if no action specified
        None => execute_list(device, json).await,

        Some(SettingsAction::List) => execute_list(device, json).await,

        Some(SettingsAction::Get { key }) => execute_get(key, device, json).await,

        Some(SettingsAction::Set { key, value }) => execute_set(key, value, device, json).await,

        Some(SettingsAction::Save { file }) => execute_save(file, device, json).await,

        Some(SettingsAction::Load { file }) => execute_load(file, device, json).await,
    }
}

/// List all settings from the device.
async fn execute_list(device: Option<&str>, json: bool) -> Result<()> {
    // Connect to device
    let dev = resolve_device(device).context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    // Send settings command (firmware supports both "settings" and "settings list")
    let response = conn
        .send_command("settings")
        .await
        .context("failed to list settings")?;

    // Parse response
    // Note: send_command() already strips the "OK" terminator, so we just get payload lines
    let lines: Vec<&str> = response.lines().collect();

    // Parse key=value lines
    let mut settings = Vec::new();
    for line in lines {
        if let Some((key, value)) = line.split_once('=') {
            // Determine access level based on known read-only fields
            // Note: This is temporary - in the future, firmware could include access level
            let access = if key == "serial_number" { "ro" } else { "rw" };

            settings.push((key.to_string(), value.to_string(), access));
        }
    }

    // Output
    if json {
        // JSON format with metadata
        let json_settings: Vec<_> = settings
            .iter()
            .map(|(key, value, access)| {
                json!({
                    "key": key,
                    "value": value,
                    "access": access
                })
            })
            .collect();

        let output = json!({
            "settings": json_settings,
            "count": settings.len()
        });

        println!("{}", json_output::format_success(output));
    } else {
        // Human-readable table format
        println!("Settings:");
        println!("  {:<20} {:<10} {}", "Key", "Access", "Value");
        println!("  {:-<20} {:-<10} {:-<30}", "", "", "");

        for (key, value, access) in settings.iter() {
            let access_str = if *access == "ro" { "RO" } else { "RW" };
            println!("  {:<20} {:<10} {}", key, access_str, value);
        }

        println!("\nTotal: {} settings", settings.len());
    }

    Ok(())
}

/// Get a specific setting value.
async fn execute_get(key: &str, device: Option<&str>, json: bool) -> Result<()> {
    let dev = resolve_device(device).context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    let cmd = format!("settings get {}", key);
    let response = conn
        .send_command(&cmd)
        .await
        .context(format!("failed to get setting '{}'", key))?;

    // send_command() already strips "OK", so response is just the value
    let value = response.trim();

    if json {
        let data = json!({
            "key": key,
            "value": value
        });
        println!("{}", json_output::format_success(data));
    } else {
        println!("{}", value);
    }
    Ok(())
}

/// Set a specific setting value.
async fn execute_set(key: &str, value: &str, device: Option<&str>, json: bool) -> Result<()> {
    let dev = resolve_device(device).context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    // Quote value if it contains spaces
    let formatted_value = if value.contains(' ') {
        format!("\"{}\"", value)
    } else {
        value.to_string()
    };

    let cmd = format!("settings set {} {}", key, formatted_value);
    let _response = conn
        .send_command(&cmd)
        .await
        .context(format!("failed to set setting '{}'", key))?;

    // send_command() returns empty string for commands that only return "OK"
    // If we got here without error, the command succeeded
    if json {
        let data = json!({
            "key": key,
            "value": value,
            "status": "success"
        });
        println!("{}", json_output::format_success(data));
    } else {
        println!("✓ Setting '{}' set to '{}'", key, value);
    }
    Ok(())
}

/// Save all settings to a JSON preset file.
async fn execute_save(file: &str, device: Option<&str>, json_output_flag: bool) -> Result<()> {
    let dev = resolve_device(device).context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    // Get list of all settings
    let response = conn
        .send_command("settings")
        .await
        .context("failed to list settings")?;

    // send_command() already strips "OK", so we just get payload lines
    let lines: Vec<&str> = response.lines().collect();

    // Parse settings
    let mut settings_array = Vec::new();

    for line in lines {
        if let Some((key, value)) = line.split_once('=') {
            // Determine access level (temporary hardcoded logic)
            let access = if key == "serial_number" { "ro" } else { "rw" };

            settings_array.push(json!({
                "key": key,
                "value": value,
                "access": access
            }));
        }
    }

    // Build JSON document
    let json_doc = json!({
        "settings": settings_array
    });

    // Write to file with pretty printing
    let json_string = serde_json::to_string_pretty(&json_doc)?;
    fs::write(file, json_string).context(format!("failed to write preset file '{}'", file))?;

    if json_output_flag {
        let data = json!({
            "file": file,
            "settings_count": settings_array.len()
        });
        println!("{}", json_output::format_success(data));
    } else {
        println!("✓ Saved {} settings to '{}'", settings_array.len(), file);
    }

    Ok(())
}

/// Load settings from a JSON preset file.
async fn execute_load(file: &str, device: Option<&str>, json_output_flag: bool) -> Result<()> {
    // 1. Read and parse JSON
    let content = fs::read_to_string(file).context(format!("Failed to read file '{}'", file))?;

    let json_value: serde_json::Value =
        serde_json::from_str(&content).context(format!("Failed to parse JSON file '{}'", file))?;

    let settings_array = json_value["settings"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("JSON file must contain 'settings' array"))?;

    // 2. Connect to device
    let dev = resolve_device(device).context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    // 3. Apply each setting
    let mut successes = Vec::new();
    let mut failures = Vec::new();
    let mut skipped = Vec::new();

    for setting_obj in settings_array {
        let key = setting_obj["key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Setting missing 'key' field"))?;

        let value = setting_obj["value"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Setting '{}' missing 'value' field", key))?;

        let access = setting_obj["access"].as_str().unwrap_or("rw");

        // Skip read-only settings
        if access == "ro" {
            skipped.push((key.to_string(), "read-only".to_string()));
            continue;
        }

        // Quote value if needed
        let formatted_value = if value.contains(' ') {
            format!("\"{}\"", value)
        } else {
            value.to_string()
        };

        let cmd = format!("settings set {} {}", key, formatted_value);
        match conn.send_command(&cmd).await {
            Ok(_response) => {
                // send_command() already validated the "OK" response
                // If we got here, the command succeeded
                successes.push((key.to_string(), value.to_string()));
            }
            Err(e) => {
                failures.push((key.to_string(), e.to_string()));
            }
        }
    }

    // 4. Report results
    if json_output_flag {
        let data = json!({
            "file": file,
            "successes": successes.len(),
            "failures": failures.len(),
            "skipped": skipped.len(),
            "details": {
                "successes": successes,
                "failures": failures,
                "skipped": skipped
            }
        });
        println!("{}", json_output::format_success(data));
    } else {
        println!("Loaded settings from '{}'", file);
        println!("  ✓ {} succeeded", successes.len());
        if !skipped.is_empty() {
            println!("  - {} skipped (read-only)", skipped.len());
        }
        if !failures.is_empty() {
            println!("  ✗ {} failed", failures.len());
            for (key, error) in &failures {
                eprintln!("    - '{}': {}", key, error);
            }
        }
    }

    // Return error if any failed
    if !failures.is_empty() {
        Err(anyhow::anyhow!("Some settings failed to apply"))
    } else {
        Ok(())
    }
}
