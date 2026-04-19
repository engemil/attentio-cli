use anyhow::{Context, Result};
use serde_json::json;

use crate::json_output;
use crate::protocol::open_client;

/// Execute the `claim` command — claim control of the device.
pub async fn execute_claim(device: Option<&str>, json: bool) -> Result<()> {
    let mut client = open_client(device).await?;

    let session_id = client.claim().await.context("failed to claim device")?;

    if json {
        let output = json!({
            "message": "Device claimed (remote mode active)",
            "session_id": session_id,
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!(
            "Device claimed. Remote mode active. Session ID: {}",
            session_id
        );
    }

    Ok(())
}

/// Execute the `release` command — release control of the device.
pub async fn execute_release(device: Option<&str>, json: bool) -> Result<()> {
    let mut client = open_client(device).await?;

    client.release().await.context("failed to release device")?;

    if json {
        let output = json!({ "message": "Device released (standalone mode)" });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Device released. Standalone mode restored.");
    }

    Ok(())
}

/// Execute the `ping` command — ping the device.
pub async fn execute_ping(device: Option<&str>, json: bool) -> Result<()> {
    let start = std::time::Instant::now();
    let mut client = open_client(device).await?;

    client.ping().await.context("failed to ping device")?;

    let elapsed = start.elapsed();

    if json {
        let output = json!({
            "message": "pong",
            "round_trip_ms": elapsed.as_millis(),
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("pong ({}ms)", elapsed.as_millis());
    }

    Ok(())
}
