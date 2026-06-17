use attentio::cli;
use attentio::device::ble::BleSelector;
use attentio::json_output;

use anyhow::{anyhow, Result};
use clap::Parser;
use serde_json::json;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Command};

/// Heuristic: a `--ble` value that looks like `AA:BB:CC:DD:EE:FF` is an address;
/// anything else is treated as an advertised device name.
fn is_mac_address(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing/logging
    let filter = if cli.verbose {
        // Include the BLE stack so a stalled D-Bus call is visible under -v.
        EnvFilter::new("attentio=trace,btleplug=debug,bluez_async=debug")
    } else {
        EnvFilter::new("attentio=warn")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    // Classify the --ble flag into a transport selector and record it globally.
    // open_client() reads this to route the serial-vs-BLE transport.
    let ble_selector = cli.ble.as_ref().map(|v| {
        if v.is_empty() {
            BleSelector::Any
        } else if let Some(n) = v.parse::<usize>().ok().filter(|&n| n >= 1) {
            BleSelector::Index(n)
        } else if is_mac_address(v) {
            BleSelector::Address(v.clone())
        } else {
            BleSelector::Name(v.clone())
        }
    });
    attentio::device::ble::set_selector(ble_selector);

    // Reject commands that have no BLE equivalent when --ble is requested.
    // dfu/dfu-enter use USB/libusb enumeration. (monitor over BLE is supported —
    // AP-only — and handled inside monitor::execute.)
    if cli.ble.is_some() {
        let unsupported = match &cli.command {
            Command::Dfu { .. } => Some("dfu"),
            Command::DfuEnter { .. } => Some("dfu-enter"),
            _ => None,
        };
        if let Some(name) = unsupported {
            let err = anyhow!("`{name}` is not supported over BLE; use USB");
            if cli.json {
                println!("{}", json_output::format_error(&err, json!({})));
                std::process::exit(1);
            }
            return Err(err);
        }
    }

    // Resolve the device serial from either the global flag or subcommand-level flag.
    // Subcommand-level --device takes precedence over the global flag.
    let global_device = cli.device.as_deref();

    let result = match &cli.command {
        Command::List => cli::commands::list::execute(cli.json).await,

        Command::Monitor { device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::monitor::execute(device).await
        }

        Command::Dfu { firmware, device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::dfu::execute(firmware, device, cli.json).await
        }

        Command::DfuEnter { device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::dfu::execute_enter(device, cli.json).await
        }

        Command::Metadata { action } => {
            cli::commands::metadata::execute(action.as_ref(), global_device, cli.json).await
        }

        Command::Settings { action } => {
            cli::commands::settings::execute(action.as_ref(), global_device, cli.json).await
        }

        // Session control
        Command::Claim => cli::commands::session::execute_claim(global_device, cli.json).await,
        Command::Release => cli::commands::session::execute_release(global_device, cli.json).await,
        Command::Ping => cli::commands::session::execute_ping(global_device, cli.json).await,

        // Device status
        Command::Status => cli::commands::status::execute(global_device, cli.json).await,

        // LED control
        Command::Set { action } => {
            cli::commands::set::execute(action, global_device, cli.json).await
        }

        // Power control
        Command::Power { action } => {
            cli::commands::power::execute(action, global_device, cli.json).await
        }

        // Runtime log level control
        Command::Loglevel { action } => {
            cli::commands::loglevel::execute(action, global_device, cli.json).await
        }

        // BLE pairing management
        Command::Ble { action } => cli::commands::ble::execute(action, cli.json).await,

        // Version info
        Command::Version => cli::commands::version::execute(cli.json),
    };

    // Handle errors: format as JSON if --json flag is set
    if let Err(e) = result {
        if cli.json {
            println!("{}", json_output::format_error(&e, json!({})));
            std::process::exit(1);
        } else {
            return Err(e);
        }
    }

    Ok(())
}
