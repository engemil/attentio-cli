use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::SetAction;
use crate::json_output;
use crate::protocol::{open_client, ApClient};

/// Execute the `set` command — set LED color, brightness, or turn off.
pub async fn execute(action: &SetAction, device: Option<&str>, json: bool) -> Result<()> {
    let mut client = open_client(device).await?;

    match action {
        SetAction::Rgb { r, g, b } => execute_rgb(&mut client, *r, *g, *b, json).await,
        SetAction::Hsv { h, s, v } => execute_hsv(&mut client, *h, *s, *v, json).await,
        SetAction::Brightness { value } => execute_brightness(&mut client, *value, json).await,
        SetAction::Off => execute_off(&mut client, json).await,
    }
}

async fn execute_rgb(client: &mut ApClient, r: u8, g: u8, b: u8, json: bool) -> Result<()> {
    client
        .set_rgb(r, g, b)
        .await
        .context(format!("failed to set RGB({}, {}, {})", r, g, b))?;

    if json {
        let output = json!({
            "r": r, "g": g, "b": b,
            "message": format!("RGB set to ({}, {}, {})", r, g, b),
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("RGB set to ({}, {}, {}).", r, g, b);
    }

    Ok(())
}

async fn execute_hsv(client: &mut ApClient, h: u16, s: u8, v: u8, json: bool) -> Result<()> {
    if h > 359 {
        anyhow::bail!("hue must be 0-359, got {}", h);
    }
    if s > 100 {
        anyhow::bail!("saturation must be 0-100, got {}", s);
    }
    if v > 100 {
        anyhow::bail!("value must be 0-100, got {}", v);
    }

    client
        .set_hsv(h, s, v)
        .await
        .context(format!("failed to set HSV({}, {}, {})", h, s, v))?;

    if json {
        let output = json!({
            "h": h, "s": s, "v": v,
            "message": format!("HSV set to ({}, {}, {})", h, s, v),
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("HSV set to ({}, {}, {}).", h, s, v);
    }

    Ok(())
}

async fn execute_brightness(client: &mut ApClient, value: u8, json: bool) -> Result<()> {
    if value > 100 {
        anyhow::bail!("brightness must be 0-100, got {}", value);
    }

    client
        .set_brightness(value)
        .await
        .context(format!("failed to set brightness to {}%", value))?;

    if json {
        let output = json!({
            "brightness": value,
            "message": format!("Brightness set to {}%", value),
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Brightness set to {}%.", value);
    }

    Ok(())
}

async fn execute_off(client: &mut ApClient, json: bool) -> Result<()> {
    client.led_off().await.context("failed to turn LEDs off")?;

    if json {
        let output = json!({ "message": "LEDs turned off" });
        println!("{}", json_output::format_success(output));
    } else {
        println!("LEDs turned off.");
    }

    Ok(())
}
