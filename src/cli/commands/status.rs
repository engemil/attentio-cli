use anyhow::{Context, Result};
use serde_json::json;

use crate::device::discovery::resolve_device;
use crate::json_output;
use crate::protocol::client::{
    control_mode_name, effects_submode_name, interface_name, standalone_mode_name,
    system_state_name,
};
use crate::protocol::ApClient;

/// Execute the `status` command — query device status.
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

    let status = client
        .get_status()
        .await
        .context("failed to get device status")?;

    let sys_state_str = system_state_name(status.system_state);
    let mode_str = control_mode_name(status.control_mode);
    let controller_str = interface_name(status.active_controller);
    let standalone_mode_str = standalone_mode_name(status.standalone_mode);
    let effects_str = effects_submode_name(status.effects_submode);

    let is_standalone = status.control_mode == 0;
    let is_effects_mode = status.standalone_mode == 4; // APP_SM_MODE_EFFECTS

    // Animated standalone modes with dynamic color (Blinking=2, Pulsation=3,
    // Effects=4). Solid Color=0, Brightness=1, Traffic Light=5, Night Light=6
    // show meaningful instantaneous RGB.
    let is_animated = is_standalone && matches!(status.standalone_mode, 2..=4);

    // Configured brightness from standalone_brightness (0-255) scaled to %.
    let configured_brightness_pct = ((status.standalone_brightness_raw as u16) * 100 / 255) as u8;

    if json {
        let output = json!({
            "system_state": sys_state_str,
            "system_state_id": status.system_state,
            "current_r": status.current_r,
            "current_g": status.current_g,
            "current_b": status.current_b,
            "brightness": status.brightness,
            "control_mode": mode_str,
            "control_mode_id": status.control_mode,
            "active_controller": controller_str,
            "active_controller_id": status.active_controller,
            "standalone_mode": standalone_mode_str,
            "standalone_mode_id": status.standalone_mode,
            "effects_submode": effects_str,
            "effects_submode_id": status.effects_submode,
            "standalone_color_index": status.standalone_color_index,
            "standalone_brightness_raw": status.standalone_brightness_raw,
            "anim_type": status.anim_type,
            "session_id": status.session_id,
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("  System state:      {}", sys_state_str);

        if is_animated {
            println!("  Color (RGB):       (dynamic)");
            println!("  Brightness:        {}%", configured_brightness_pct);
        } else {
            println!(
                "  Color (RGB):       ({}, {}, {})",
                status.current_r, status.current_g, status.current_b
            );
            println!("  Brightness:        {}%", status.brightness);
        }

        println!("  Control mode:      {}", mode_str);

        if is_standalone {
            println!("  Standalone mode:   {}", standalone_mode_str);
            if is_effects_mode {
                println!("  Active effect:     {}", effects_str);
            }
        } else {
            println!("  Active controller: {}", controller_str);
            println!("  Session ID:        {}", status.session_id);
        }
    }

    Ok(())
}
