# Attentio CLI

CLI tool for AttentioLight-1 (AL-1) device management.

## Usage

```bash
attentio list                              # List connected devices
attentio list --json                       # JSON output (for scripting)
attentio send <cmd> [--device <serial>]    # One-shot command
attentio shell [--device <serial>]         # Interactive ChibiOS shell
attentio monitor [--device <serial>]       # TUI dashboard (CDC0 + CDC1)
attentio led <mode> [options]              # LED mode/settings
attentio settings get <key>                # Read setting
attentio settings set <key> <value>        # Write setting
attentio settings load <file.toml>         # Apply preset
attentio settings save <file.toml>         # Export settings
attentio dfu <firmware.bin>                # Flash firmware
attentio dfu-enter                         # Enter bootloader mode
attentio completions <shell>               # Generate shell completions
```

### Global Flags

| Flag | Description |
|------|-------------|
| `-d, --device <serial>` | Target device by serial number (defaults to only connected device) |
| `--json` | Output results as JSON for scripting |
| `-v, --verbose` | Enable verbose/debug output |

### Implementation Status

| Command | Status |
|---------|--------|
| `list` | Done |
| `send`, `shell` | Done |
| `monitor` | Planned (Phase 3) |
| `led` | Planned (Phase 4) |
| `settings` | Planned (Phase 5) |
| `dfu`, `dfu-enter` | Planned (Phase 6) |
| `completions` | Planned |

## Setup

### Rust Toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### System Dependencies

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
cargo run -- list                    # Run during development
cargo run -- list --json             # Dev run with flags
```

### Install Locally

```bash
cargo install --path .               # Installs 'attentio' to ~/.cargo/bin
```

### udev Rules (Linux, optional)

For non-root USB access, create `/etc/udev/rules.d/99-attentio.rules`:
```
SUBSYSTEM=="usb", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666"
SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666", SYMLINK+="attentio-%s{serial}"
```

Reload rules:
```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
```

**NB!** Script available for this, `scripts/udev_rules_attentio.sh`.

## License

MIT License, see `LICENSE`-file for details.
