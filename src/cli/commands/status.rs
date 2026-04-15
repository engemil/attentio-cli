use anyhow::{Context, Result};
use serde_json::json;

use crate::device::discovery::resolve_device;
use crate::json_output;
use crate::protocol::client::{
    control_mode_name, effects_submode_name, interface_name, standalone_mode_name,
    system_state_name,
};
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

    let sys_state_str = system_state_name(state.system_state);
    let mode_str = control_mode_name(state.control_mode);
    let controller_str = interface_name(state.active_controller);
    let standalone_mode_str = standalone_mode_name(state.standalone_mode);
    let effects_str = effects_submode_name(state.effects_submode);

    let is_standalone = state.control_mode == 0;
    let is_effects_mode = state.standalone_mode == 4; // APP_SM_MODE_EFFECTS

    // Animated standalone modes with dynamic color (Blinking=2, Pulsation=3,
    // Effects=4). Solid Color=0, Brightness=1, Traffic Light=5, Night Light=6
    // show meaningful instantaneous RGB.
    let is_animated = is_standalone && matches!(state.standalone_mode, 2..=4);

    // Configured brightness from standalone_brightness (0-255) scaled to %.
    let configured_brightness_pct = ((state.standalone_brightness_raw as u16) * 100 / 255) as u8;

    if json {
        let output = json!({
            "system_state": sys_state_str,
            "system_state_id": state.system_state,
            "current_r": state.current_r,
            "current_g": state.current_g,
            "current_b": state.current_b,
            "brightness": state.brightness,
            "control_mode": mode_str,
            "control_mode_id": state.control_mode,
            "active_controller": controller_str,
            "active_controller_id": state.active_controller,
            "standalone_mode": standalone_mode_str,
            "standalone_mode_id": state.standalone_mode,
            "effects_submode": effects_str,
            "effects_submode_id": state.effects_submode,
            "standalone_color_index": state.standalone_color_index,
            "standalone_brightness_raw": state.standalone_brightness_raw,
            "anim_type": state.anim_type,
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
                state.current_r, state.current_g, state.current_b
            );
            println!("  Brightness:        {}%", state.brightness);
        }

        println!("  Control mode:      {}", mode_str);

        if is_standalone {
            println!("  Standalone mode:   {}", standalone_mode_str);
            if is_effects_mode {
                println!("  Active effect:     {}", effects_str);
            }
        } else {
            println!("  Active controller: {}", controller_str);
        }
    }

    Ok(())
}
