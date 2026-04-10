pub mod commands;

use clap::{Parser, Subcommand};

/// CLI tool for device management.
#[derive(Debug, Parser)]
#[command(name = "attentio", version, about, long_about = None)]
pub struct Cli {
    /// Target device by serial number or index from 'attentio list' (defaults to only connected device).
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
    /// List connected device(s).
    List,

    /// Launch TUI to monitor CDC debug print stream.
    Tui {
        /// Target device by serial number or index.
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

    /// Flash firmware via DFU.
    Dfu {
        /// Path to firmware binary file.
        firmware: String,

        /// Target device by serial number or index.
        #[arg(long, short)]
        device: Option<String>,
    },

    /// Enter DFU bootloader mode on the device.
    #[command(visible_alias = "bootloader-enter")]
    DfuEnter {
        /// Target device by serial number or index.
        #[arg(long, short)]
        device: Option<String>,
    },

    /// Query device metadata (firmware version, build info, etc.).
    Metadata,

    /// Manage device settings.
    Settings {
        #[command(subcommand)]
        action: Option<SettingsAction>,
    },
}

/// Subcommands for `attentio settings`.
#[derive(Debug, Subcommand)]
pub enum SettingsAction {
    /// List all settings with their current values.
    List,

    /// Get the value of a single setting.
    Get {
        /// Setting key name (e.g. "device_name", "loglevel").
        key: String,
    },

    /// Set the value of a setting.
    Set {
        /// Setting key name.
        key: String,
        /// New value.
        value: String,
    },

    /// Save all settings to a JSON file.
    Save {
        /// Output file path.
        file: String,
    },

    /// Load settings from a JSON file and apply them to the device.
    Load {
        /// Input file path.
        file: String,
    },
}
