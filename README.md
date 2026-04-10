# Attentio CLI

CLI tool for Attentio device(s) and device management. Designed to be interactive either by intuitive commands (e.g. `attentio help`) or by using the `tui` command for a real-time TUI dashboard monitoring the CDC debug print stream.

**NB!** Tested only on Ubuntu 24.04 

## Table of Contents

- [Setup](#setup)
    - [Build & Run](#build--run)
    - [Install Locally](#install-locally)
    - [udev Rules for Linux (optional)](#udev-rules-for-linux-optional)
- [Usage](#usage)
    - [TUI](#tui)
    - [Commands](#commands)
      - [Global Flags](#global-flags)
- [License](#license)

## Setup

**Rust Toolchain**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**System Dependencies**

**Ubuntu/Debian:**
```bash
sudo apt install build-essential pkg-config libudev-dev libusb-1.0-0-dev
```

**Fedora/RHEL:** (Not-yet-tested)
```bash
sudo dnf install gcc pkgconf-pkg-config systemd-devel libusb1-devel
```

**Arch:** (Not-yet-tested)
```bash
sudo pacman -S base-devel pkgconf libusb
```

**Alpine:** (Not-yet-tested)
```bash
apk add build-base pkgconf eudev-dev libusb-dev
```

**macOS:** (Not-yet-tested)
```bash
brew install libusb
```

**Windows:** (Not-yet-tested)
- Install libusb via [vcpkg](https://vcpkg.io/) or [libusb.info](https://libusb.info)
- Install WinUSB driver with [Zadig](https://zadig.akeo.ie/)


### Build & Run

Compile project

```bash
cargo build --release    # Release build
cargo build              # Debug build
```

Compile and run in one step with e.g. `help` command:
```bash
cargo run --release -- help  # Release build and run
cargo run -- help            # Debug build and run
```

Clean up:
```bash
cargo clean
```

### Install Locally

```bash
cargo install --path .               # Installs 'attentio' to ~/.cargo/bin
# Run commandsm, e.g. attentio help
```

Clean up local install
```bash
cargo uninstall attentio
```

### udev Rules for Linux (optional)

For non-root serial port access, the easiest approach is to run the provided script on the
**host OS** (not inside a container):

```bash
sudo ./scripts/udev_rules_attentio.sh
```

The script creates udev rules in `/etc/udev/rules.d/99-attentio.rules` and adds the
current user to the `dialout` and `plugdev` groups (log out and back in for group changes
to take effect).

**Permission problem in Linux**

Without udev rules / group membership you will get **permission denied**:
```
WARN Failed to open debug port /dev/ttyACM1: Permission denied
```

## Usage

### TUI

Debug prints dashboard: full-height view of CDC0 debug output stream.

- **Auto-reconnect** — retries every 3 s when the port is unavailable or disconnects mid-session
- **Port-busy detection** — if another process (minicom, etc.) holds the port, the pane indicates that the port is busy and retries until it is released

**TUI Control**
- **PageUp** / **PageDown** to scroll
- **Up** / **Down** to scroll one line at a time
- **Esc** / **CTRL** + **C** to quit


### Commands

```bash
# Device discovery
attentio [--json] list                                                  # List connected devices (index, name, type, status, serial, ports)

# Device info
attentio [--json] metadata [--device <#|serial>]                        # Query device metadata (firmware version, build date, platform, etc.)
attentio [--json] status [--device <#|serial>]                          # Query device state (color, brightness, mode, controller)

# Session control
attentio [--json] claim [--device <#|serial>]                           # Claim control (enter remote mode)
attentio [--json] release [--device <#|serial>]                         # Release control (return to standalone mode)
attentio [--json] ping [--device <#|serial>]                            # Ping device (keep-alive check)
attentio [--json] session [--device <#|serial>]                         # Show session info (mode, active controller)

# LED control (auto-claims if needed)
attentio [--json] set rgb <r> <g> <b> [--device <#|serial>]             # Set LED color (RGB 0-255)
attentio [--json] set hsv <h> <s> <v> [--device <#|serial>]             # Set LED color (H:0-359, S:0-100, V:0-100)
attentio [--json] set brightness <val> [--device <#|serial>]            # Set LED brightness (0-100%)
attentio [--json] set off [--device <#|serial>]                         # Turn LEDs off

# Power control (auto-claims if needed)
attentio [--json] power on [--device <#|serial>]                        # Wake from low-power mode
attentio [--json] power off [--device <#|serial>]                       # Enter low-power mode

# Settings (set/load auto-claim if needed)
attentio [--json] settings list [--device <#|serial>]                   # List all device settings
attentio [--json] settings get <key> [--device <#|serial>]              # Get a single setting value
attentio [--json] settings set <key> <value> [--device <#|serial>]      # Set a setting value
attentio [--json] settings save <file.json> [--device <#|serial>]       # Save all settings to JSON file
attentio [--json] settings load <file.json> [--device <#|serial>]       # Load settings from JSON file and apply

# Interactive
attentio tui [--device <#|serial>]                                      # TUI dashboard (debug prints, auto-reconnect)

# Firmware update
attentio [--json] dfu <firmware.bin> [--device <#|serial>]              # Flash firmware via DFU (auto-enters bootloader if needed)
attentio [--json] dfu-enter [--device <#|serial>]                       # Enter DFU bootloader mode
attentio [--json] bootloader-enter [--device <#|serial>]                # Same as "dfu-enter"

# Version
attentio --version                                                      # Print CLI version (flag)
attentio [--json] version                                               # Print CLI version (subcommand)
```

**Note:** Commands that modify device state (LED, power, settings set) require a _claim_.
The CLI auto-claims transparently on first use. The claim stays active until you run
`attentio release` or disconnect.

#### Settings File Format

The `settings save` and `settings load` commands use a simple JSON file:

```json
{
  "device_name": "MyAttentioLight",
  "loglevel": "2"
}
```

### Global Flags

| Flag | Description |
|------|-------------|
| `-d, --device <#\|serial>` | Target device by index (from `attentio list`) or USB serial number (defaults to only connected device) |
| `--json` | Output results as JSON with `status` field (`OK` or `ERROR`) for scripting/automation |
| `-v, --verbose` | Enable verbose/debug output |



## License

MIT License, see `LICENSE`-file for details.
