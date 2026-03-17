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

### Implemented Commands

```bash
attentio list                                               # List connected devices and their serial number
attentio --json list                                        # List connected devices with JSON output
attentio send <cmd> [args...] [--device <serial>]           # One-shot command (supports quoted arguments)
attentio --json send <cmd> [args...] [--device <serial>]    # One-shot with JSON output
attentio shell [--device <serial>]                          # Interactive ChibiOS shell (<serial> can be found from 'attentio list')
attentio tui [--device <serial>]                            # TUI dashboard (dual CDC, auto-reconnect)
attentio led <mode> [options]                               # LED mode/settings (TO DO: planned)
attentio settings                                           # List all settings (defaults to list)
attentio settings list                                      # List all settings
attentio settings get <key>                                 # Read setting
attentio settings set <key> <value>                         # Write setting
attentio settings load <file.json>                          # Apply preset from JSON file
attentio settings save <file.json>                          # Export settings to JSON file
attentio dfu <firmware.bin>                                 # Enter bootloader mode and flash application firmware (TO DO: planned)
attentio dfu-enter                                          # Enter bootloader mode
attentio bootloader-enter                                   # Same as "dfu-enter"
attentio completions <shell>                                # Generate shell completions (planned)
```

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


### Quoting Arguments

The `send` command automatically handles arguments with spaces. Both double quotes (`"`) and single quotes (`'`) work identically:

```bash
# All of these work:
attentio send echo test                                     # Single word, no quotes needed
attentio send echo "test"                                   # Single word with quotes (quotes removed)
attentio send echo 'test'                                   # Single word with single quotes (same as above)
attentio send echo "test this"                              # Multi-word argument (quotes preserved)
attentio send echo 'test this'                              # Multi-word with single quotes (same result)
```

**How it works:** When you use quotes in your shell command, bash treats the quoted text as a single argument. The CLI detects arguments containing spaces and automatically wraps them in quotes when sending to the device's shell.

**Note:** Arguments with embedded quotes (e.g., `'He said "hello"'`) are escaped but may not work correctly due to limitations in the ChibiOS shell parser.

### Global Flags

| Flag | Description |
|------|-------------|
| `-d, --device <serial>` | Target device by serial number (defaults to only connected device) |
| `--json` | Output results as JSON with `status` field (`OK` or `ERROR`) for scripting/automation |
| `-v, --verbose` | Enable verbose/debug output |

### JSON Output Format

The `--json` flag provides structured output for scripting and automation. All commands that support `--json` use a consistent format for both success and error cases.

#### Success Response

All successful operations return a JSON object with `"status": "OK"` and additional fields depending on the command:

**Example: `attentio --json send version`**
```json
{
  "status": "OK",
  "device": "AL1MB1-12345678",
  "command": "version",
  "response": "1.2.3"
}
```

**Example: `attentio --json list`**
```json
{
  "status": "OK",
  "data": [
    {
      "serial": "AL1MB1-12345678",
      "product": "AttentioLight-1",
      "cdc0": {
        "path": "/dev/ttyACM0",
        "role": "debug"
      },
      "cdc1": {
        "path": "/dev/ttyACM1",
        "role": "shell"
      }
    }
  ]
}
```

#### Error Response

All errors return a JSON object with `"status": "ERROR"` and error details:

**Example: Device not found**
```json
{
  "status": "ERROR",
  "error": "no device(s) found",
  "error_type": "DeviceNotFound"
}
```

**Example: Command error with context**
```json
{
  "status": "ERROR",
  "error": "protocol error: unknown command",
  "error_type": "Protocol",
  "command": "badcmd",
  "protocol_message": "unknown command"
}
```

**Example: Multiple devices found**
```json
{
  "status": "ERROR",
  "error": "multiple devices found — use --device <serial> to select one: AL1MB1-111, AL1MB1-222",
  "error_type": "MultipleDevices",
  "available_devices": ["AL1MB1-111", "AL1MB1-222"]
}
```

#### Error Types

(NB! This is subject for change)

The `error_type` field can be one of:
- `DeviceNotFound` - No devices connected
- `MultipleDevices` - Multiple devices found, need to specify `--device`
- `DeviceSerialNotFound` - Specified serial number not found
- `PortBusy` - Port is already open by another process
- `Protocol` - Device returned an error or invalid response
- `Timeout` - Command timed out waiting for response
- `Serial` - Serial port communication error
- `Io` - I/O error
- `Other` - Other errors

#### Parsing JSON Output

**Bash/Shell:**
```bash
# Check if command succeeded
if attentio --json send version | jq -e '.status == "OK"' > /dev/null; then
    echo "Success"
fi

# Extract response
attentio --json send version | jq -r '.response'

# Handle errors
output=$(attentio --json send badcmd)
if echo "$output" | jq -e '.status == "ERROR"' > /dev/null; then
    echo "Error: $(echo "$output" | jq -r '.error')"
fi
```

**Python:**
```python
import subprocess
import json

result = subprocess.run(
    ['attentio', '--json', 'send', 'version'],
    capture_output=True,
    text=True
)

data = json.loads(result.stdout)
if data['status'] == 'OK':
    print(f"Version: {data['response']}")
else:
    print(f"Error ({data['error_type']}): {data['error']}")
```

**Node.js:**
```javascript
const { execSync } = require('child_process');

try {
    const output = execSync('attentio --json send version', { encoding: 'utf8' });
    const data = JSON.parse(output);
    
    if (data.status === 'OK') {
        console.log(`Version: ${data.response}`);
    } else {
        console.error(`Error: ${data.error}`);
    }
} catch (err) {
    console.error('Failed to execute command');
}
```


## License

MIT License, see `LICENSE`-file for details.
