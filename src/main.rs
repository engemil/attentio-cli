mod cli;
mod device;
mod error;
mod json_output;
mod protocol;
mod tui;

use anyhow::Result;
use clap::Parser;
use serde_json::json;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing/logging
    let filter = if cli.verbose {
        EnvFilter::new("attentio=trace")
    } else {
        EnvFilter::new("attentio=warn")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    // Resolve the device serial from either the global flag or subcommand-level flag.
    // Subcommand-level --device takes precedence over the global flag.
    let global_device = cli.device.as_deref();

    let result = match &cli.command {
        Command::List => cli::commands::list::execute(cli.json).await,

        Command::Tui { device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::tui::execute(device).await
        }

        Command::Led { mode, options } => {
            cli::commands::led::execute(mode, options, global_device, cli.json).await
        }

        Command::Dfu { firmware, device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::dfu::execute(firmware, device, cli.json).await
        }

        Command::DfuEnter { device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::dfu::execute_enter(device, cli.json).await
        }

        Command::Metadata => cli::commands::metadata::execute(global_device, cli.json).await,

        Command::Settings { action } => {
            cli::commands::settings::execute(action.as_ref(), global_device, cli.json).await
        }
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
