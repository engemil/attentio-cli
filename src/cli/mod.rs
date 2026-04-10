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
    Metadata {
        #[command(subcommand)]
        action: Option<MetadataAction>,
    },

    /// Manage device settings.
    Settings {
        #[command(subcommand)]
        action: Option<SettingsAction>,
    },

    /// Claim control of the device (enter remote mode).
    Claim,

    /// Release control of the device (return to standalone mode).
    Release,

    /// Ping the device (keep-alive check).
    Ping,

    /// Show session info (control mode and active controller).
    Session,

    /// Query device status (state, color, brightness, mode).
    Status,

    /// Set LED color, brightness, or turn LEDs off.
    Set {
        #[command(subcommand)]
        action: SetAction,
    },

    /// Control device power.
    Power {
        #[command(subcommand)]
        action: PowerAction,
    },

    /// Print CLI version information.
    Version,
}

/// Subcommands for `attentio metadata`.
#[derive(Debug, Subcommand)]
pub enum MetadataAction {
    /// List all metadata fields (default when no subcommand given).
    List,

    /// Get the value of a single metadata field.
    Get {
        /// Metadata key name (e.g. "firmware_version", "serial_number", "uptime").
        key: String,
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

/// Subcommands for `attentio set`.
#[derive(Debug, Subcommand)]
pub enum SetAction {
    /// Set LED color using RGB values (0-255 each).
    Rgb {
        /// Red value (0-255).
        r: u8,
        /// Green value (0-255).
        g: u8,
        /// Blue value (0-255).
        b: u8,
    },

    /// Set LED color using HSV values.
    Hsv {
        /// Hue (0-359).
        h: u16,
        /// Saturation (0-100).
        s: u8,
        /// Value/brightness (0-100).
        v: u8,
    },

    /// Set LED brightness (0-100%).
    Brightness {
        /// Brightness percentage (0-100).
        value: u8,
    },

    /// Turn LEDs off.
    Off,
}

/// Subcommands for `attentio power`.
#[derive(Debug, Subcommand)]
pub enum PowerAction {
    /// Power on (wake from low-power mode).
    On,
    /// Power off (enter low-power mode).
    Off,
}
