# Attentio CLI

CLI tool for Attentio device(s) and device management. Designed to be interactive either by intuitive commands (e.g. `attentio help`), send shell command directly (e.g. `attentio send help`), or by using the `tui` command, real-time TUI dashboard with dual CDC (shell and serial prints).

**NB!** Tested only on Ubuntu 24.04 

## Table of Contents

- [Setup](#setup)
    - [Build & Run](#build--run)
    - [Install Locally](#install-locally)
    - [udev Rules for Linux (optional)](#udev-rules-for-linux-optional)
- [Usage](#usage)
    - [TUI](#tui
    - [Commands](#commands)
      - [Global Flags](#global-flags)
      - [Settings File Format](#settings-file-format)
      - [Quoting Arguments](#quoting-arguments)
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
WARN Failed to open shell port /dev/ttyACM2: Permission denied
```

## Usage

### TUI

Split-pane dashboard: debug prints (CDC0) on top, interactive shell (CDC1) on bottom.

- **Auto-reconnect** — retries every 3 s when a port is unavailable or disconnects mid-session
- **Port-busy detection** — if another process (minicom, etc.) holds a port, the pane indicates that port is busy and retries until the port is released
- **Idle disconnect detection** — Detects if any of the ports gets disconnected even when the
  shell is idle. Clearify marked by reconnecting info.

**TUI Control**
- **PageUp** / **PageDown** to scroll each pane
- **Up** / **Down** to recall previous commands
- **Tab** to switch focus between debug and shell panes
- **Esc** / **CTRL** + **C** to quit


### Commands

```bash
attentio [--json] list                                              # List connected devices (serial, type, status, ports/USB)
attentio [--json] send <cmd> [args...] [--device <serial>]          # One-shot command (supports quoted arguments)
attentio led <mode> [options] [--device <serial>]                   # LED mode/settings (planned)
attentio [--json] metadata [--device <serial>]                      # List all device metadata (read-only identity/build info)
attentio [--json] metadata get <key> [--device <serial>]            # Read a specific metadata field
attentio [--json] settings [--device <serial>]                      # List all settings (defaults to list)
attentio [--json] settings list [--device <serial>]                 # List all settings
attentio [--json] settings get <key> [--device <serial>]            # Read setting
attentio [--json] settings set <key> <value> [--device <serial>]    # Write setting
attentio [--json] settings load <file.json> [--device <serial>]     # Apply preset from JSON file
attentio [--json] settings save <file.json> [--device <serial>]     # Export settings to JSON file
attentio shell [--device <serial>]                                  # Interactive ChibiOS shell (<serial> can be found from 'attentio list')
attentio tui [--device <serial>]                                    # TUI dashboard (dual CDC, auto-reconnect)
attentio dfu <firmware.bin> [--device <serial>]                     # Flash firmware via DFU (auto-enters bootloader if needed)
attentio dfu-enter [--device <serial>]                              # Enter DFU bootloader mode
attentio bootloader-enter [--device <serial>]                       # Same as "dfu-enter"
attentio completions <shell> [--device <serial>]                    # Generate shell completions (planned)
```

### Global Flags

| Flag | Description |
|------|-------------|
| `-d, --device <serial>` | Target device by serial number (defaults to only connected device) |
| `--json` | Output results as JSON with `status` field (`OK` or `ERROR`) for scripting/automation |
| `-v, --verbose` | Enable verbose/debug output |

### Settings File Format

The `settings load` and `settings save` commands use a JSON file with the following structure:

```json
{
  "settings": [
    { "key": "device_name", "value": "MyDevice" },
    { "key": "example_setting", "value": "100" }
  ]
}
```

Each entry in the `settings` array contains:
- `key` — the setting name
- `value` — the setting value (always a string)

### Quoting Arguments

The `send` command automatically handles arguments with spaces. Both double quotes (`"`) and single quotes (`'`) work identically:

```bash
# All of these work:
attentio send echo test           # Single word, no quotes needed
attentio send echo "test"         # Single word with quotes (quotes removed)
attentio send echo 'test'         # Single word with single quotes (same as above)
attentio send echo "test this"    # Multi-word argument (quotes preserved)
attentio send echo 'test this'    # Multi-word with single quotes (same result)
```

**Note:** Arguments with embedded quotes (e.g., `'He said "hello"'`) are escaped but may not work correctly due to limitations in the ChibiOS shell parser.



## License

MIT License, see `LICENSE`-file for details.
