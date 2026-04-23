mod cli;
mod device;
mod error;
mod json_output;
mod monitor;
mod protocol;

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
