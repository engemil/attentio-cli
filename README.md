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
attentio [--json] list                                                  # List connected devices (index, name, type, status, serial, ports)
attentio [--json] metadata [--device <#|serial>]                        # Query device metadata (firmware version, build date, platform, etc.)
attentio [--json] settings list [--device <#|serial>]                   # List all device settings
attentio [--json] settings get <key> [--device <#|serial>]              # Get a single setting value
attentio [--json] settings set <key> <value> [--device <#|serial>]      # Set a setting value
attentio [--json] settings save <file.json> [--device <#|serial>]       # Save all settings to JSON file
attentio [--json] settings load <file.json> [--device <#|serial>]       # Load settings from JSON file and apply to device
attentio led <mode> [options] [--device <#|serial>]                     # LED mode/settings (planned)
attentio tui [--device <#|serial>]                                      # TUI dashboard (debug prints, auto-reconnect)
attentio dfu <firmware.bin> [--device <#|serial>]                       # Flash firmware via DFU (auto-enters bootloader if needed)
attentio dfu-enter [--device <#|serial>]                                # Enter DFU bootloader mode
attentio bootloader-enter [--device <#|serial>]                         # Same as "dfu-enter"
```

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
