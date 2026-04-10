use anyhow::{Context, Result};
use serde_json::json;

use crate::device::discovery::resolve_device;
use crate::json_output;
use crate::protocol::client::{control_mode_name, interface_name};
use crate::protocol::ApClient;

/// Execute the `claim` command — claim control of the device.
pub async fn execute_claim(device: Option<&str>, json: bool) -> Result<()> {
    let mut client = open_client(device).await?;

    client.claim().await.context("failed to claim device")?;

    if json {
        let output = json!({ "message": "Device claimed (remote mode active)" });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Device claimed. Remote mode active.");
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

/// Execute the `session` command — show session info.
pub async fn execute_session(device: Option<&str>, json: bool) -> Result<()> {
    let mut client = open_client(device).await?;

    let session = client
        .get_session()
        .await
        .context("failed to get session info")?;

    let mode_str = control_mode_name(session.mode);
    let controller_str = interface_name(session.active_controller);

    if json {
        let output = json!({
            "mode": mode_str,
            "mode_id": session.mode,
            "active_controller": controller_str,
            "active_controller_id": session.active_controller,
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("  Mode:              {}", mode_str);
        println!("  Active controller: {}", controller_str);
    }

    Ok(())
}

/// Open an AP client for the resolved device.
async fn open_client(device: Option<&str>) -> Result<ApClient> {
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;

    let port_path = dev
        .ap_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no protocol port", dev.serial))?
        .to_string();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    ApClient::open(&port_path).context(format!("failed to open protocol port {}", port_path))
}
