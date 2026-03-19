# Changelog

All notable changes to the Attentio CLI (`attentio`) project will be documented in this file.

**Version Format:** MAJOR.MINOR.PATCH
- **MAJOR:** Incompatible API/protocol changes
- **MINOR:** New features (backward compatible)
- **PATCH:** Bug fixes (backward compatible)

[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Note: Update `Cargo.toml` when publishing new version.

---

## [Development] (2026-03-19)

Added

- **DFU firmware flashing (`dfu <firmware.bin>`)** — full implementation with:
  - Firmware header validation (magic, VID/PID, size checks)
  - Auto-enters bootloader mode if device is running normal application
  - Progress bars for erase and flash phases (spinner + percentage)
  - Post-flash verification (waits for device to reboot into normal mode)
  - USB device reset and retry logic for stale DFU state recovery
- **`dfu-enter` command implementation** — sends `dfu` shell command to reboot device into
  bootloader mode, then polls until DFU device re-enumerates on USB.
- **`metadata` command** — read-only device identity and build information:
  - `metadata` or `metadata list` — lists all metadata fields
  - `metadata get <key>` — reads a specific metadata field value
- **Device mode detection** — distinguishes Normal (application) vs Bootloader (DFU) mode.
  The `list` command now shows a STATUS column indicating the device's operational mode.
- **USB location in device discovery** — `list` output now includes USB bus/device location
  (e.g., "Bus 001 Device 060") for physical identification when multiple devices are connected.
- **Pure DFU device detection via `rusb`** — devices in bootloader mode without serial ports
  are now detected directly via USB enumeration, not just serial port discovery.
- **Device type vs device name** — `list` output now distinguishes between USB product string
  (DEVICE TYPE, e.g., "AttentioLight-1") and user-assigned name (DEVICE NAME from settings).
- **Shell synchronization** — new `sync_shell()` method waits for USB CDC link to stabilize,
  drains stale buffer data, and detects the ChibiOS shell prompt before sending commands.
- New dependencies: `rusb` (USB enumeration), `dfu-libusb` (DFU flashing), `indicatif` (progress bars).
- **Command alias `bootloader-enter` for `dfu-enter`** — both commands now work interchangeably 
  to enter DFU bootloader mode. The alias is visible in help output for better discoverability.
- **Auto-reconnection for TUI** — both CDC0 (debug) and CDC1 (shell) ports now
  automatically retry every 3 seconds when a port is unavailable at startup or disconnects
  mid-session. The TUI shows "(reconnecting...)" in yellow instead of "(not connected)".
- **Exclusive serial port access with `TIOCEXCL`** — after opening via manual `libc::open()`
  + termios configuration, the port is claimed exclusively so that future processes cannot
  open it while attentio holds it.
- **Port-busy detection via `/proc` scan** — before opening a serial port, scans
  `/proc/*/fd/` to check if any other process already has the device open. Returns a clear
  `PortBusy` error.
- **`PortBusy` error variant** in `AttentioError` with `is_port_busy()` helper, fully wired
  into the serial port open logic.
- **TUI "port busy" status** — when a port is held by another process, the TUI pane shows
  `(PORT BUSY)` in the title and `(port busy — close other process)` in red. A background
  reconnect task retries every 3 seconds; when the other process releases the port, the pane
  automatically connects.
- New dependency: `libc` (POSIX serial port open).
- **`monitor` TUI command** - real-time dashboard with dual CDC view.
  - Horizontal split layout: debug prints (CDC0) on top, interactive shell (CDC1) on bottom.
  - Async architecture: background reader tasks for both CDC ports with mpsc channels.
  - Input line with cursor navigation, backspace/delete, home/end.
  - Command history with up/down arrow recall.
  - Scrollable panes with PageUp/PageDown.
  - Tab to switch focus between debug and shell panes.
  - Graceful single-CDC fallback: shell-only TUI when device has no separate debug port.
  - Status bar with device serial, focus indicator, and key hints.
  - Clean terminal restore on exit (Esc/Ctrl+C).
- **TUI module** (`src/tui/`) with separated concerns: `app.rs` (state), `ui.rs` (rendering), `event.rs` (input handling).
- New dependencies: `ratatui`, `crossterm` (with event-stream).
- **Settings management commands** — fully implemented `settings` command with five operations:
  - `settings` or `settings list` — lists all device settings in table or JSON format
  - `settings get <key>` — reads a single setting value
  - `settings set <key> <value>` — writes a setting (auto-quotes values with spaces)
  - `settings save <file.json>` — exports all current settings to JSON preset file
  - `settings load <file.json>` — imports and applies settings from JSON preset file with smart handling (skips read-only fields, continues on partial failures, reports detailed status)
- **JSON preset format** for settings.

Changed

- **`list` command output reorganized** — new columns: DEVICE TYPE (USB product string),
  DEVICE NAME (user-assigned), STATUS (Normal/Bootloader), USB LOCATION. The previous
  "TYPE" column (dual/single CDC) is removed.
- **udev rules updated** — added `ENV{ID_MM_DEVICE_IGNORE}="1"` to prevent ModemManager
  from probing Attentio devices and interfering with serial communication.
- **`resolve_device()` is now async** — device resolution now queries metadata and settings
  from normal-mode devices to populate serial number and device name fields.
- **Clean port release on exit** — `DeviceConnection` drop clears `TIOCEXCL` via `TIOCNXCL`
  before close, ensuring the port is immediately available to the next opener.
- **`send` command now accepts multi-word arguments** — command arguments no longer need 
  quotes; e.g., `attentio send help config` instead of `attentio send "help config"`. 
  Arguments are automatically joined with spaces. JSON output now includes a `"status"` 
  field, and non-JSON mode prints "OK" after the response.
- **Smart argument quoting for `send` command** — arguments containing spaces are now 
  automatically wrapped in double quotes when sent to the device. Both `"quoted"` and 
  `'quoted'` arguments work identically (bash removes the quotes, CLI re-adds them for 
  device shell compatibility). Examples: `attentio send echo "test this"` and 
  `attentio send echo 'test this'` both work correctly. Includes comprehensive unit tests 
  for the quoting logic.
- **Renamed `monitor` command to `tui`** — the command is now invoked as `attentio tui` for 
  clarity. All documentation and internal references updated accordingly. The command still 
  provides the same functionality: TUI dashboard for monitoring CDC data streams.
- **TUI starts even with busy ports** — if one or both CDC ports are busy at startup, the
  TUI launches with the busy pane(s) showing the red status while available ports work normally.
- **Shell disconnect detection while idle** — the shell I/O task now performs health-check
  reads while waiting for user input, so USB cable pulls are detected promptly (within ~5 s)
  instead of only on the next command attempt.
- **Improved `send_command()` protocol handling**:
  - Echo line skipping — ChibiOS echoes back the sent command; the first received line
    matching the sent text is now silently discarded.
  - Stale buffer draining before sending — clears leftover prompt bytes from previous commands.
  - Inter-line timeout (300 ms) — handles commands (like ChibiOS `help`) that print output
    without an `OK`/`ERROR` terminator.
  - Partial response return on hard timeout — returns collected lines instead of erroring
    when data was received but the terminator never arrived.
- TUI pane titles and status messages now distinguish "reconnecting..." (yellow) from
  "not connected" (gray).
- Updated README:
  - Added implementation status table.
  - Marked unimplemented commands as `(planned)` in usage section.
  - Added `monitor` feature summary (auto-reconnect, exclusive serial with busy detection, scrolling, history).
  - Rewrote udev section: recommend running the script, added `dialout` group note,
    moved manual steps into collapsible details block.
  - Clarified `--json` scope (currently `list` and `send`).
  - Added "Settings File Format" section documenting JSON preset structure.
- Improved TUI terminal setup/cleanup — raw mode and alternate screen init wrapped so
  cleanup always runs even if setup fails.
- Commented out unused port mappings in `.devcontainer/docker-compose.yml`.
- Removed `#[allow(dead_code)]` from `debug_port()`, `read_line()`, and `with_timeout()` — now used by TUI.
- Updated README implementation status: TUI command marked as Done.
- **CLI framework** with clap (derive) supporting subcommands: `list`, `send`, `shell`, `monitor`, `led`, `settings`, `dfu`, and `dfu-enter`.
- **Global flags**: `--device <serial>`, `--json`, and `--verbose`.
- **Device discovery** module for enumerating and selecting connected AttentioLight-1 devices via serial ports.
- **Device connection** module for async serial communication using tokio-serial.
- **Error handling** with anyhow and thiserror.
- **Tracing/logging** with tracing-subscriber and env-filter support.
- `list` and `send`/`shell` commands implemented (Phase 1-2 complete).
- **Cargo project setup** with dependencies: clap, tokio, tokio-serial, serialport, serde, serde_json, anyhow, thiserror, tracing.
- `.gitignore` for Rust target directory.
- `rustfmt.toml` formatter configuration.
- **README** with full CLI usage reference, implementation status table, setup instructions for Linux/macOS/Windows, and build/install commands.
- **udev rules script** (`scripts/udev_rules_attentio.sh`) for Linux USB device access.
- Future plan items: unit tests for discovery logic and CI/CD pipeline.
- Expanded README from placeholder to full documentation.
- Updated `docs/PLAN.md` with future items (unit tests, CI/CD).

Fixed

- **JSON error output with `--json` flag** — errors from all commands are now properly 
  formatted as JSON when `--json` is set. Previously, errors propagated to the default 
  handler and printed in human-readable format even with `--json`. Centralized error 
  handling in `main.rs` ensures consistent `{"status": "ERROR", ...}` output.

- Fixed typo in udev script filename (`udev_rules_attetio.sh` → `udev_rules_attentio.sh`).

