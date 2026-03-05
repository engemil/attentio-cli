# THE PLAN

CLI tool for AttentioLight-1 (AL-1) device management.

### Implementation Status

| Command | Status |
|---------|--------|
| `list` | Done |
| `send`, `shell` | Done |
| `monitor` | Done |
| `led` | Planned (Phase 4) |
| `settings` | Planned (Phase 5) |
| `dfu`, `dfu-enter` | Planned (Phase 6) |
| `completions` | Planned |


## Stack

| Layer | Crate | Purpose |
|-------|-------|---------|
| CLI | clap (derive) | Args, subcommands, completions |
| Serial | tokio-serial | Async CDC/ACM I/O |
| Async | tokio | Concurrent I/O, TUI responsiveness |
| Interactive | rustyline | Line editing, history |
| TUI | ratatui | Dashboard/monitor mode |
| DFU | dfu-libusb | Native firmware updates |
| Config | serde + toml | Settings, presets |
| Output | serde_json | `--json` for scripting |
| Errors | anyhow + thiserror | Ergonomic error handling |
| Logging | tracing | `--verbose` debug output |
| Signals | ctrlc | Graceful Ctrl+C shutdown |
| Progress | indicatif | DFU flash progress bar |

## Commands

```
attentio list                              # List connected devices
attentio shell [--device <serial>]         # Interactive ChibiOS shell
attentio send <cmd> [--device <serial>]    # One-shot command
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

Global flags: `--device <serial>`, `--json`, `--verbose`

## Implementation Order

1. **Core** - clap setup, device discovery (VID/PID), async serial connection
2. **Basic comms** - `list`, `send`, `shell`
3. **TUI** - `monitor` with dual CDC view (prints + shell)
4. **LED** - `led` command
5. **Settings** - get/set, TOML presets, config management commands
6. **DFU** - dfu-libusb integration, `dfu-enter`

## Device Protocol

- **VID/PID:** `0x0483:0xdf11` (EngEmil.io AttentioLight-1)
- **CDC0:** Debug prints (read-only stream)
- **CDC1:** Shell commands (request/response)
- **Format:** `<cmd>\r\n` -> response until `OK\r\n` or `ERROR <msg>\r\n`

## Multi-device

- Enumerate by VID/PID, differentiate by serial number
- `--device` flag selects target (defaults to first/only device)

## Config (~/.config/attentio/config.toml)

```toml
default_device = "ABC123"

[presets.demo]
led_mode = "pulse"
brightness = 80
```

## Firmware Tasks (attentiolight-1-firmware)

Required changes to support CLI:

| Task | Description |
|------|-------------|
| **Dual CDC** | Add CDC1 for shell commands (CDC0 = debug prints) |
| **Shell commands** | Implement ChibiOS shell handlers for CLI interaction |
| **`version`** | Return firmware version from app header |
| **`led <mode> [opts]`** | Set LED mode, color, brightness, speed |
| **`settings get <key>`** | Read from EFL (serial, name, mode, etc.) |
| **`settings set <key> <val>`** | Write to EFL with validation |
| **`dfu`** | Trigger reboot into bootloader DFU mode |
| **Serial number** | Store unique ID in EFL, return via `settings get serial` |
| **Response format** | Standardize: `OK\r\n` or `ERROR <msg>\r\n` |

See [shell_commands.md](shell_commands.md) for the full command protocol specification.

## udev Rule (optional)

For non-root USB access and stable device paths, add `/etc/udev/rules.d/99-attentio.rules`:
```
# USB access for DFU
SUBSYSTEM=="usb", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666"

# Serial port access + stable symlink
SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666", SYMLINK+="attentio-%s{serial}"
```

Then: `sudo udevadm control --reload-rules && sudo udevadm trigger`

## Dependencies

### Rust Dependencies (Cargo.toml)

```toml
[dependencies]
# CLI
clap = { version = "4", features = ["derive"] }

# Async runtime
tokio = { version = "1", features = ["full"] }
tokio-serial = "5"

# Interactive shell
rustyline = "14"

# TUI
ratatui = "0.28"
crossterm = "0.28"

# DFU
dfu-libusb = "0.5"

# Config & serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Error handling
anyhow = "1"
thiserror = "1"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Utilities
ctrlc = "3"
indicatif = "0.17"
```

### Cross-Platform Compatibility

| Crate | Linux | macOS | Windows | Notes |
|-------|:-----:|:-----:|:-------:|-------|
| `clap` | ✅ | ✅ | ✅ | Pure Rust |
| `tokio` | ✅ | ✅ | ✅ | Pure Rust |
| `tokio-serial` | ✅ | ✅ | ✅ | Native OS APIs (no libserialport) |
| `rustyline` | ✅ | ✅ | ✅ | Pure Rust |
| `ratatui` + `crossterm` | ✅ | ✅ | ✅ | Pure Rust |
| `dfu-libusb` | ✅ | ✅ | ✅ | Requires libusb |
| `serde`, `toml`, `serde_json` | ✅ | ✅ | ✅ | Pure Rust |
| `anyhow`, `thiserror` | ✅ | ✅ | ✅ | Pure Rust |
| `tracing` | ✅ | ✅ | ✅ | Pure Rust |
| `ctrlc`, `indicatif` | ✅ | ✅ | ✅ | Pure Rust |

**Note:** `tokio-serial` uses native OS APIs (Linux: `termios`, macOS: `IOKit`, Windows: Win32) - no `libserialport` needed.

### System Dependencies

**Linux (Ubuntu/Debian):**
```bash
sudo apt install build-essential pkg-config libudev-dev libusb-1.0-0-dev
```

**Linux (Fedora/RHEL):**
```bash
sudo dnf install gcc pkgconf-pkg-config systemd-devel libusb1-devel
```

**Linux (Arch):**
```bash
sudo pacman -S base-devel pkgconf libusb
```

**Linux (Alpine):**
```bash
apk add build-base pkgconf eudev-dev libusb-dev
```

**macOS:**
```bash
brew install libusb
```

**Windows:**
- Install libusb via [vcpkg](https://vcpkg.io/) or download from [libusb.info](https://libusb.info)
- Install WinUSB driver for DFU device (use [Zadig](https://zadig.akeo.ie/))

### Rust Toolchain

```bash
# Install Rust (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# For static musl builds (optional, portable binary)
rustup target add x86_64-unknown-linux-musl
```

### Build Commands

```bash
cargo build              # Development
cargo build --release    # Release

# Static binary (any Linux distro)
cargo build --release --target x86_64-unknown-linux-musl
```

## Config Management (Phase 5)

User config lives at `~/.config/attentio/config.toml`. Additional commands for managing it:

- `attentio config path` — print the config directory location
- `attentio config reset` — delete user config files
- Document manual removal in README uninstall section as a fallback

## Future

- Unit tests for discovery logic (`devices_from_ports`, `select_device`) and connection handling
- CI/CD pipeline (GitHub Actions: build, clippy, rustfmt, tests)
- Post-build data injection script and included in CLI tool
- USB HID/Bulk alternatives
- USB wake-up from host
