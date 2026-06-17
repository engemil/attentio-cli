pub mod commands;

use clap::{Parser, Subcommand};

/// CLI tool for device management.
#[derive(Debug, Parser)]
#[command(name = "attentio", version, about, long_about = None)]
pub struct Cli {
    /// Target device by serial number or index from 'attentio list' (defaults to only connected device).
    #[arg(long, short, global = true)]
    pub device: Option<String>,

    /// Connect over BLE instead of USB. Optionally pin a device with
    /// `--ble=<name|MAC>`; bare `--ble` connects to the single advertised
    /// AttentioLight-1.
    #[arg(long, global = true, num_args = 0..=1, require_equals = true, default_missing_value = "")]
    pub ble: Option<String>,

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

    /// Launch monitor to view CDC serial print stream.
    Monitor {
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

    /// Get or set the runtime log level (ephemeral, lost on reboot).
    ///
    /// Controls runtime log verbosity. Changes take effect immediately but are
    /// NOT saved to flash. Use `attentio settings set default_loglevel <N>` to
    /// change the persistent default that survives reboots.
    ///
    /// Levels: 0=NONE, 1=ERROR, 2=WARN, 3=INFO, 4=DEBUG
    Loglevel {
        #[command(subcommand)]
        action: LoglevelAction,
    },

    /// Manage BLE pairing (host bond) for a device.
    Ble {
        #[command(subcommand)]
        action: BleAction,
    },

    /// Print CLI version information.
    Version,
}

/// Subcommands for `attentio ble`.
#[derive(Debug, Subcommand)]
pub enum BleAction {
    /// Pair (bond) with a BLE device.
    Pair {
        /// Target: BD_ADDR, advertised name, or index from `attentio list`.
        /// Omit to pair the single advertised AttentioLight-1.
        target: Option<String>,
    },

    /// Unpair (remove the host bond) from a BLE device.
    Unpair {
        /// Target: BD_ADDR, advertised name, or index from `attentio list`.
        /// Omit for the single advertised AttentioLight-1.
        target: Option<String>,
    },
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
        /// Setting key name (e.g. "device_name", "default_loglevel").
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

/// Subcommands for `attentio loglevel`.
#[derive(Debug, Subcommand)]
pub enum LoglevelAction {
    /// Get the current runtime log level.
    Get,

    /// Set the runtime log level (0-4). Lost on reboot.
    ///
    /// Levels: 0=NONE, 1=ERROR, 2=WARN, 3=INFO, 4=DEBUG.
    /// This change is ephemeral. For persistent changes, use:
    ///   attentio settings set default_loglevel <N>
    Set {
        /// Log level (0=NONE, 1=ERROR, 2=WARN, 3=INFO, 4=DEBUG).
        level: u8,
    },
}
