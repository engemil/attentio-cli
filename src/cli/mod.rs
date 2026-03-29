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

    /// Open an interactive ChibiOS shell session.
    Shell {
        /// Target device by serial number or index.
        #[arg(long, short)]
        device: Option<String>,
    },

    /// Send a one-shot command to the device.
    ///
    /// Arguments containing spaces are automatically quoted when sent to the device.
    ///
    /// Examples:
    ///   attentio send help
    ///   attentio send echo test
    ///   attentio send echo "test this"
    ///   attentio send echo 'test this'
    ///   attentio send led pulse red
    Send {
        /// The command to send (multiple args joined with spaces).
        ///
        /// Arguments with spaces are automatically quoted for the device shell.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,

        /// Target device by serial number or index.
        #[arg(long, short)]
        device: Option<String>,
    },

    /// Launch TUI to monitor CDC data streams (debug prints + shell).
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

    /// Read device metadata (read-only identity and build info).
    Metadata {
        /// Metadata action (defaults to 'list' if not specified).
        #[command(subcommand)]
        action: Option<MetadataAction>,
    },

    /// Read or write device settings.
    Settings {
        /// Settings action (defaults to 'list' if not specified).
        #[command(subcommand)]
        action: Option<SettingsAction>,
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
}

#[derive(Debug, Subcommand)]
pub enum MetadataAction {
    /// List all metadata fields (default action).
    List,

    /// Read a metadata field value.
    Get {
        /// Metadata field key to read.
        key: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SettingsAction {
    /// List all settings (default action).
    List,

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

    /// Apply settings from a JSON preset file.
    Load {
        /// Path to JSON preset file.
        file: String,
    },

    /// Export current settings to a JSON file.
    Save {
        /// Path to output JSON file.
        file: String,
    },
}
