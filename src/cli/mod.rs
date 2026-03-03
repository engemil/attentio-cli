pub mod commands;

use clap::{Parser, Subcommand};

/// CLI tool for AttentioLight-1 (AL-1) device management.
#[derive(Debug, Parser)]
#[command(name = "attentio", version, about, long_about = None)]
pub struct Cli {
    /// Target device by serial number (defaults to only connected device).
    #[arg(long, short, global = true)]
    pub device: Option<String>,

    /// Output results as JSON for scripting.
    #[arg(long, global = true)]
    pub json: bool,

    /// Enable verbose/debug output.
    #[arg(long, short, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List connected AttentioLight-1 devices.
    List,

    /// Open an interactive ChibiOS shell session.
    Shell {
        /// Target device by serial number.
        #[arg(long, short)]
        device: Option<String>,
    },

    /// Send a one-shot command to the device.
    Send {
        /// The command to send.
        cmd: String,

        /// Target device by serial number.
        #[arg(long, short)]
        device: Option<String>,
    },

    /// Open TUI dashboard with dual CDC view (debug prints + shell).
    Monitor {
        /// Target device by serial number.
        #[arg(long, short)]
        device: Option<String>,
    },

    /// Control LED mode and settings.
    Led {
        /// LED mode (e.g. pulse, solid, rainbow).
        mode: String,

        /// Additional LED options (color, brightness, speed).
        #[arg(trailing_var_arg = true)]
        options: Vec<String>,
    },

    /// Read or write device settings.
    Settings {
        #[command(subcommand)]
        action: SettingsAction,
    },

    /// Flash firmware via DFU.
    Dfu {
        /// Path to firmware binary file.
        firmware: String,
    },

    /// Enter DFU bootloader mode on the device.
    DfuEnter,

    /// Generate shell completions.
    Completions {
        /// Target shell (bash, zsh, fish, powershell).
        shell: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SettingsAction {
    /// Read a setting value.
    Get {
        /// Setting key to read.
        key: String,
    },

    /// Write a setting value.
    Set {
        /// Setting key to write.
        key: String,

        /// Value to set.
        value: String,
    },

    /// Apply settings from a TOML preset file.
    Load {
        /// Path to TOML preset file.
        file: String,
    },

    /// Export current settings to a TOML file.
    Save {
        /// Path to output TOML file.
        file: String,
    },
}
