use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::BleAction;
use crate::device::ble::{self, BleSelector};
use crate::json_output;

/// `AA:BB:CC:DD:EE:FF`-shaped?
fn is_mac_address(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Classify a `ble` command target into a [`BleSelector`], mirroring how the
/// global `--ble` flag is parsed (index → address → name; empty/none → any).
fn parse_target(target: Option<&str>) -> BleSelector {
    match target {
        None | Some("") => BleSelector::Any,
        Some(v) => {
            if let Some(n) = v.parse::<usize>().ok().filter(|&n| n >= 1) {
                BleSelector::Index(n)
            } else if is_mac_address(v) {
                BleSelector::Address(v.to_string())
            } else {
                BleSelector::Name(v.to_string())
            }
        }
    }
}

/// Execute an `attentio ble <pair|unpair>` command.
pub async fn execute(action: &BleAction, json: bool) -> Result<()> {
    match action {
        BleAction::Pair { target } => {
            let address = ble::resolve_address(&parse_target(target.as_deref()))
                .await
                .context("could not resolve a BLE device to pair")?;
            ble::pair(&address).await.context("failed to pair")?;
            report(json, "paired", &address);
        }
        BleAction::Unpair { target } => {
            let address = ble::resolve_address(&parse_target(target.as_deref()))
                .await
                .context("could not resolve a BLE device to unpair")?;
            ble::unpair(&address).await.context("failed to unpair")?;
            report(json, "unpaired", &address);
        }
    }
    Ok(())
}

fn report(json: bool, verb: &str, address: &str) {
    if json {
        let output = json!({
            "message": format!("{verb} {address}"),
            "address": address,
        });
        println!("{}", json_output::format_success(output));
    } else {
        println!("Device {address} {verb}.");
    }
}
