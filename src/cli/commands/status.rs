use anyhow::{Context, Result};
use serde_json::json;

use crate::device::discovery::resolve_device;
use crate::json_output;
use crate::protocol::client::{control_mode_name, interface_name};
use crate::protocol::ApClient;

/// Execute the `status` command — query device state.
pub async fn execute(device: Option<&str>, json: bool) -> Result<()> {
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;

    let port_path = dev
        .ap_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no protocol port", dev.serial))?
        .to_string();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = ApClient::open(&port_path)
        .context(format!("failed to open protocol port {}", port_path))?;

    let state = client
        .get_state()
        .await
        .context("failed to get device state")?;

    let mode_str = control_mode_name(state.control_mode);
    let controller_str = interface_name(state.active_controller);

    if json {
        let output = json!({
            "system_state": state.system_state,
            "current_r": state.current_r,
            "current_g": state.current_g,
            "current_b": state.current_b,
            "brightness": state.brightness,
            "control_mode": mode_str,
            "control_mode_id": state.control_mode,
            "active_controller": controller_str,
            "active_controller_id": state.active_controller,
            "standalone_mode": state.standalone_mode,
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("  System state:      {}", state.system_state);
        println!(
            "  Color (RGB):       ({}, {}, {})",
            state.current_r, state.current_g, state.current_b
        );
        println!("  Brightness:        {}%", state.brightness);
        println!("  Control mode:      {}", mode_str);
        println!("  Active controller: {}", controller_str);
        println!("  Standalone mode:   {}", state.standalone_mode);
    }

    Ok(())
}
