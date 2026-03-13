# Attentio CLI

CLI tool for AttentioLight-1 (AL-1) device management. Designed to be interactive either by sending the commands directly (e.g. `attentio send help`) or by using the `tui` command, real-time TUI dashboard with dual CDC (shell and serial prints).

**NB!** Tested on Ubuntu 24.04 (not yet tested on alternative distros, nor operative systems).

## Table of Contents

- [Setup](#setup)
    - [Build & Run](#build--run)
    - [Install Locally](#install-locally)
    - [udev Rules for Linux (optional)](#udev-rules-for-linux-optional)
- [Usage](#usage)
    - [Global Flags](#global-flags)
    - [TUI Usage](#tui-usage)
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

**Fedora/RHEL:**
```bash
sudo dnf install gcc pkgconf-pkg-config systemd-devel libusb1-devel
```

**Arch:**
```bash
sudo pacman -S base-devel pkgconf libusb
```

**Alpine:**
```bash
apk add build-base pkgconf eudev-dev libusb-dev
```

**macOS:**
```bash
brew install libusb
```

**Windows:**
- Install libusb via [vcpkg](https://vcpkg.io/) or [libusb.info](https://libusb.info)
- Install WinUSB driver with [Zadig](https://zadig.akeo.ie/)

### Build & Run

```bash
cargo build --release                # Release build
# Run commands: cargo run -- <command>
cargo run -- list                    # Test
```

Clean up:
```bash
cargo clean
```

### Install Locally

```bash
cargo install --path .               # Installs 'attentio' to ~/.cargo/bin
# Run commands: attentio <command>
attentio list                        # Test
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

**Manual setup (without the script)**

Create `/etc/udev/rules.d/99-attentio.rules`:
```
SUBSYSTEM=="usb", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666"
SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666", SYMLINK+="attentio-%s{serial}"
```

Reload rules and add yourself to the `dialout` group:
```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG dialout $USER
```

**Permission problem in Linux**

Without udev rules / group membership you will get **permission denied**:
```
WARN Failed to open debug port /dev/ttyACM1: Permission denied
WARN Failed to open shell port /dev/ttyACM2: Permission denied
```

## Usage

```bash
attentio list                                               # List connected devices
attentio list --json                                        # JSON output
attentio send <cmd> [args...] [--device <serial>]           # One-shot command (e.g., 'attentio send help')
attentio send --json <cmd> [args...] [--device <serial>]    # One-shot with JSON output
attentio shell [--device <serial>]                          # Interactive ChibiOS shell (serial from 'list')
attentio tui [--device <serial>]                            # TUI dashboard (dual CDC, auto-reconnect)
attentio led <mode> [options]                               # LED mode/settings (planned)
attentio settings get <key>                                 # Read setting (planned)
attentio settings set <key> <value>                         # Write setting (planned)
attentio settings load <file.toml>                          # Apply preset (planned)
attentio settings save <file.toml>                          # Export settings (planned)
attentio dfu <firmware.bin>                                 # Enter bootloader mode and flash application firmware (planned)
attentio dfu-enter                                          # Enter bootloader mode
attentio bootloader-enter                                   # Same as "dfu-enter"
attentio completions <shell>                                # Generate shell completions (planned)
```

### Global Flags

| Flag | Description |
|------|-------------|
| `-d, --device <serial>` | Target device by serial number (defaults to only connected device) |
| `--json` | Output results as JSON (currently used by `list` and `send`) |
| `-v, --verbose` | Enable verbose/debug output |

(More info will come, to show better use-case)


### TUI Usage

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

## License

MIT License, see `LICENSE`-file for details.
