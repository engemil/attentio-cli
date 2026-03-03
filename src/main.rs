mod cli;
mod device;
mod error;

use anyhow::Result;
use clap::Parser;
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

    match &cli.command {
        Command::List => {
            cli::commands::list::execute(cli.json)?;
        }

        Command::Send { cmd, device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::send::execute(cmd, device, cli.json).await?;
        }

        Command::Shell { device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::shell::execute(device).await?;
        }

        Command::Monitor { device } => {
            let device = device.as_deref().or(global_device);
            cli::commands::monitor::execute(device).await?;
        }

        Command::Led { mode, options } => {
            cli::commands::led::execute(mode, options, global_device, cli.json).await?;
        }

        Command::Settings { action } => {
            cli::commands::settings::execute(action, global_device, cli.json).await?;
        }

        Command::Dfu { firmware } => {
            cli::commands::dfu::execute(firmware).await?;
        }

        Command::DfuEnter => {
            cli::commands::dfu::execute_enter(global_device).await?;
        }

        Command::Completions { shell } => {
            cli::commands::completions::execute(shell)?;
        }
    }

    Ok(())
}
