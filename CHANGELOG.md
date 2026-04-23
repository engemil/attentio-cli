# Changelog

All notable changes to the Attentio CLI (`attentio`) project will be documented in this file.

**Version Format:** MAJOR.MINOR.PATCH
- **MAJOR:** Incompatible API/protocol changes
- **MINOR:** New features (backward compatible)
- **PATCH:** Bug fixes (backward compatible)

[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Note: Update `Cargo.toml` when publishing new version.

---

## [Development] (2026-04-23)

Added

- **Shared UI/Monitor Helpers** — consolidated `render_ap_pane` and `render_serial_pane` 
  into a unified `render_pane` helper using a new `PaneView` structure, reducing code 
  duplication in the TUI renderer.

Changed

- **Optimized Device Client Connection** — added `open_client_for_device` to 
  `protocol/client.rs` which bypasses redundant device resolution when the `AttentioDevice`
  is already known. Used by `metadata` and `settings` commands to speed up execution.
- **Refactored DFU USB Matching** — extracted `find_matching_attentio_usb_device` to 
  reduce duplication across `open_dfu_by_serial`, `reset_dfu_device`, and 
  `wait_for_dfu_device_sync` in `dfu.rs`.
- **Refactored Monitor Port Handling** — extracted `try_open_port` in `monitor.rs` 
  for cleaner connection state management and error handling.
- **Simplified AP Parser** — removed redundant `data_idx` from `ApParser` state; it now 
  reliably uses `data.len()` for payload progress tracking.
- **Version Command Sync** — made the `version` command synchronous since it no longer 
  performs async I/O.
- Removed automatic sorting by serial number in `devices_from_ports` discovery.

Fixed

- **DFU Success Handling** — fixed a false-positive `rusb: Input/Output Error` or
  `Pipe` error that occurred immediately after successfully flashing a device.
  The error was caused by the device automatically dropping off the USB bus to
  reboot into the newly flashed firmware. The CLI now catches these expected USB
  drop errors during the manifestation phase and treats them as a successful flash.
- **Firmware CRC32 Calculation** — fixed a bug where the CLI computed the CRC32 of the
  firmware payload including 224 bytes of padding, causing a mismatch against properly
  signed firmware. The CLI now correctly hashes the firmware starting from the
  `VECTOR_TABLE_OFFSET` (`0x100`) as expected by the bootloader.

---

## [Development] (2026-04-19)

Changed

- **Extracted shared `open_client()` helper** — deduplicated device resolution and AP
  client creation from 7 command files (`session`, `metadata`, `settings`, `status`,
  `set`, `power`, `loglevel`) into a single `open_client()` function in
  `src/protocol/client.rs`, re-exported from `src/protocol/mod.rs`.

- **Unified DFU device wait functions** — replaced separate `wait_for_dfu_device` and
  `wait_for_normal_device` with a single `wait_for_device_mode()` function in `dfu.rs`.

- **Centralized log level name mapping** — made `level_name()` and `LEVEL_NAMES` public
  in `loglevel.rs`; `monitor.rs`, `format.rs`, and `ui.rs` now use the shared mapping
  instead of maintaining their own copies.

- **Extracted `is_attentio_device()` helper** — deduplicated Attentio VID/PID matching
  from 5 call sites in `dfu.rs` and `discovery.rs` into `config.rs`.

- **Composed `find_devices()` from `find_devices_fast()`** — removed duplicated port
  enumeration logic; `find_devices` now delegates to `find_devices_fast` and adds
  metadata enrichment.

- **Fixed `find_dfu_only_devices` error type** — now returns `AttentioError` instead of
  `String` for consistency with the rest of the error handling.

Fixed

- **Removed double error reporting** in `list.rs` and `dfu.rs` — errors were being
  printed with `eprintln!` and then propagated with `?`, causing duplicate output.
  Now uses `?` propagation only.

Removed

- **Dead code cleanup** — removed unnecessary `#[allow(dead_code)]` on event constants
  in `packet.rs`; removed dead re-exports with `#[allow(unused_imports)]` in
  `device/mod.rs`.

- **Renamed `json_flag` to `json`** in `version.rs` for consistency with other commands.

---

## [Development] (2026-04-19)

Added

- **Session ID support** — the CLI now parses and displays the session ID assigned by
  the device on CLAIM.
  - `claim()` returns the `u16` session ID from the CLAIM OK response (2-byte big-endian
    payload). `attentio claim` prints the session ID in both plain text and JSON output.
  - `DeviceStatus` gains a `session_id` field, parsed from GET_STATUS response bytes
    12-13 (big-endian). `attentio status` shows the session ID when in REMOTE mode;
    JSON output always includes it.
  - Monitor's SESSION_END event display now includes the session ID when present in the
    3-byte event payload (`← EVT_SESSION_END TAKEOVER (session 3)`).

---

## [Development] (2026-04-19)

Added

- **Two-pane monitor layout** — `attentio monitor` now shows two panes:
  - **Top pane:** AP protocol traffic (CDC1) — incoming responses (`← OK`, `← ERROR`),
    device events (`← EVT_BUTTON SHORT_PRESS`, `← EVT_SESSION_END RELEASED`), and
    outgoing commands (`→ LOG_SET_LEVEL [3 (INFO)]`). Bidirectional: commands sent from
    other terminal sessions are also captured.
  - **Bottom pane:** serial prints (CDC0) — unchanged from before.
  - **Tab** to switch focused pane; scroll keys apply to the focused pane.
  - Status bar shows `Focus:AP` / `Focus:SER` indicator.

- **Persistent CDC1 reader** — the monitor now maintains a persistent connection to the
  AP port (CDC1) with a background reader/writer task. Incoming AP packets are parsed
  via `ApParser` and formatted for display. The connection supports auto-reconnect and
  port-busy detection, matching the existing CDC0 behavior.

- **Unified AP connection for log-level hotkeys** — pressing 1-4 to change log level
  now sends commands through the persistent CDC1 connection (via an async channel) instead
  of opening a one-shot connection each time. Both the outgoing command and the device
  response appear in the AP pane.

- **AP packet formatting module (`src/monitor/format.rs`)** — human-readable formatting
  for all AP protocol traffic: command names, RGB/HSV/brightness payloads, button event
  types, session end reasons, and error codes.

Changed

- **Renamed "debug" to "serial" throughout CDC0 code** — all internal identifiers, enum
  variants, function names, and comments referring to CDC0 as "debug" now use "serial"
  instead: `DebugPrints` → `SerialPrints`, `debug_port()` → `serial_port()`,
  `debug_connected` → `serial_connected`, `push_debug_line` → `push_serial_line`,
  `Pane::Debug` → `Pane::Serial`, `debug_reader_task` → `serial_reader_task`, etc.
  The `list` command port role label changes from `[debug]` to `[serial]`.

---

## [Development] (2026-04-17)

Added

- **`attentio loglevel get/set` command** — new command for runtime (ephemeral) log level
  control via the `LOG_GET_LEVEL` (0x60) and `LOG_SET_LEVEL` (0x61) protocol commands.
  Changes take effect immediately but are lost on reboot. For persistent changes, use
  `attentio settings set default_loglevel <N>`.

- **Monitor log level control** — press `1`-`4` in the monitor to change the runtime log level
  on the fly (1=ERROR, 2=WARN, 3=INFO, 4=DEBUG). Current level is shown in the status
  bar with color-coded display. Initial level is queried from the device at startup.

Changed

- **Renamed settings key `loglevel` to `default_loglevel`** — the persistent log level
  setting is now called `default_loglevel` to distinguish it from the runtime log level.
  This is a **breaking change** for scripts using `attentio settings set loglevel`.

- **Renamed `tui` command to `monitor`** — the command is now `attentio monitor`. Better
  reflects its purpose as a monitoring tool for the CDC serial print stream.

- **Faster monitor exit** — ESC/Ctrl+C now exits within ~400ms max instead of potentially
  seconds. Background tasks are aborted concurrently with a single shared timeout instead
  of sequentially.

- **Renamed "Debug Prints" to "Serial Prints"** — all CDC0 references in the monitor
  UI, command help text, and documentation now use "Serial Prints" instead of
  "Debug Prints".

---

## [Development] (2026-04-15)

Fixed

- **`list` command timeout** — `attentio list` now enforces a 5-second overall timeout
  on device enumeration (`find_devices()`). Previously, USB enumeration and per-device
  AP queries could hang indefinitely if the OS-level USB calls stalled or multiple devices
  each hit the 3-second AP response timeout. Returns `AttentioError::Timeout` on expiry.
  The `version` command is excluded — it is a purely local operation with no I/O.

Added

- **Human-readable status output** — `attentio status` now displays human-readable
  names for all state fields instead of raw numeric IDs:
  - System state: BOOT, POWERUP, ACTIVE, POWERDOWN, OFF (was raw number).
  - Standalone mode: Solid Color, Brightness, Blinking, Pulsation, Effects,
    Traffic Light, Night Light (was raw number).
  - Active effect: Rainbow, Color Cycle, Breathing, Candle, Fire, Lava Lamp,
    Day/Night, Ocean, Northern Lights, Thunder Storm, Police, Health Pulse,
    Memory (new field, shown when standalone mode is Effects).
  Added `system_state_name()`, `standalone_mode_name()`, and
  `effects_submode_name()` mapping functions in `client.rs`.

- **Context-sensitive status display** — the `status` command now adapts its
  output based on the current control mode:
  - In STANDALONE: shows standalone mode name and active effect (if in Effects mode).
  - In REMOTE: shows active controller (USB, BLE, WiFi).
  - For animated standalone modes (Blinking, Pulsation, Effects): color shows
    `(dynamic)` and brightness shows the configured standalone brightness level
    instead of the fluctuating instantaneous animation values.

- **Expanded `DeviceState` struct** — added `effects_submode`, `standalone_color_index`,
  `standalone_brightness_raw`, and `anim_type` fields. These are parsed from the
  new 12-byte `GET_STATE` response (firmware update required). Backward compatible
  with 8-byte responses from older firmware (new fields default to 0).

- **Protocol cleanup: renamed GET_STATE to GET_STATUS, removed GET_SESSION** —
  `CMD_GET_STATE` renamed to `CMD_GET_STATUS` (0x40). `DeviceState` struct renamed
  to `DeviceStatus`. `get_state()` client method renamed to `get_status()`.
  `CMD_GET_SESSION` (0x42) removed along with `SessionInfo` struct, `get_session()`
  method, and `attentio session` CLI command. Session info (control mode, active
  controller) is already included in the `GET_STATUS` response and displayed by
  `attentio status`.

Changed

- **`status` JSON output enriched** — JSON output now includes human-readable names
  alongside raw IDs for all fields: `system_state`/`system_state_id`,
  `standalone_mode`/`standalone_mode_id`, `effects_submode`/`effects_submode_id`,
  plus new raw fields `standalone_color_index`, `standalone_brightness_raw`, and
  `anim_type`.

---

## [Development] (2026-04-10)

Added

- **`metadata list` / `metadata get <key>` subcommands** — metadata command now supports
  `list` (default) and `get <key>` subcommands, mirroring the `settings` command pattern.
  `metadata list` fetches all metadata fields using paginated `GET_METADATA` (0x43).
  `metadata get <key>` queries a single field using the new `METADATA_GET` (0x44) command.
  Both support `--json` output.

- **Paginated metadata protocol support** — `get_metadata()` loops over firmware pages
  using the new `[total_count][page][page_count][KV pairs]` wire format. Added
  `parse_kv_paginated()` parser and `get_metadata_field()` client method.

- **`CMD_METADATA_GET` (0x44) protocol constant** — new command ID for single-field
  metadata query in `packet.rs`.

- **`version` subcommand** — `attentio version` prints the CLI version (`attentio <version>`).
  Supports `--json` flag for machine-readable output (`{"status":"OK","version":"..."}`).
  The existing `attentio --version` flag continues to work as before via clap.

- **Session control commands** — `claim`, `release`, `ping`, and `session`:
  - `attentio claim` — sends AP CLAIM (0x01) to take control of the device (enters remote mode).
  - `attentio release` — sends AP RELEASE (0x02) to return device to standalone mode.
  - `attentio ping` — sends AP PING (0x03) keep-alive, reports round-trip time in ms.
  - `attentio session` — sends AP GET_SESSION (0x42) to display control mode and active controller.

- **LED control commands** — `set rgb`, `set hsv`, `set brightness`, `set off`:
  - `attentio set rgb <r> <g> <b>` — sends AP SET_RGB (0x21) with 3-byte payload.
  - `attentio set hsv <h> <s> <v>` — sends AP SET_HSV (0x22) with 4-byte payload (H little-endian u16, S/V u8).
  - `attentio set brightness <val>` — sends AP SET_BRIGHTNESS (0x23) with 1-byte percentage (0-100).
  - `attentio set off` — sends AP LED_OFF (0x20) to turn LEDs off.

- **Power control commands** — `power on`, `power off`:
  - `attentio power on` — sends AP POWER_ON (0x10) to wake from low-power mode.
  - `attentio power off` — sends AP POWER_OFF (0x11) to enter low-power mode.

- **Device status command** — `attentio status` sends AP GET_STATE (0x40) and displays
  system state, current RGB, brightness, control mode, active controller, and standalone mode.

- **Auto-claim for claim-required commands** — the `ApClient` tracks claim state and
  automatically sends CLAIM before commands that require it (LED control, power control,
  settings set). The claim is kept active until explicitly released. This fixes the
  `ERR_NOT_CONTROLLER` (0x01) error that occurred when running `settings set` without
  a prior claim.

- **AP command constants** — added all missing command IDs to `packet.rs`: `CMD_CLAIM` (0x01),
  `CMD_RELEASE` (0x02), `CMD_PING` (0x03), `CMD_POWER_ON` (0x10), `CMD_POWER_OFF` (0x11),
  `CMD_LED_OFF` (0x20), `CMD_SET_RGB` (0x21), `CMD_SET_HSV` (0x22), `CMD_SET_BRIGHTNESS` (0x23),
  `CMD_GET_STATE` (0x40), `CMD_GET_SESSION` (0x42), and event IDs `CMD_EVT_BUTTON` (0x80),
  `CMD_EVT_STATE_CHANGE` (0x81), `CMD_EVT_SESSION_END` (0x82).

- **`ApClient` high-level methods** — `claim()`, `release()`, `ping()`, `ensure_claimed()`,
  `get_state()`, `get_session()`, `set_rgb()`, `set_hsv()`, `set_brightness()`, `led_off()`,
  `power_on()`, `power_off()`. Includes `DeviceState` and `SessionInfo` response structs.

Changed

- **`settings set` now auto-claims** — `ApClient::settings_set()` calls `ensure_claimed()`
  internally, so `attentio settings set loglevel 4` works without a manual `attentio claim` first.

Removed

- **`led` command stub** — replaced by `set rgb`, `set hsv`, `set brightness`, and `set off`
  commands matching the design document command matrix.

---

## [Development] (2026-04-10)

Added

- **Attentio Protocol (AP) client library (`src/protocol/`)** — new module implementing
  the client side of the AP for communicating with the device over CDC1:
  - `crc.rs` — CRC-8/CCITT lookup table and `crc8()` function (identical to firmware table).
  - `packet.rs` — protocol constants (SYNC byte, command IDs, error codes), `build_packet()`
    for constructing AP packets, `ApParser` byte-at-a-time state machine for parsing responses,
    and `ApResponse` type.
  - `client.rs` — high-level `ApClient` wrapping `DeviceConnection` with typed methods:
    `send_command()`, `get_metadata()`, `settings_list()`, `settings_get()`, `settings_set()`.
    Includes binary key-value payload parsing for count-prefixed lists and single-pair responses.
  - Unit tests for CRC-8 computation, packet building, parser state machine (including garbage
    prefix handling and CRC mismatch rejection).

- **`read_raw()` method on `DeviceConnection`** — reads raw bytes from the serial port for
  the AP response parser. Complements the existing `write_raw()` and `read_line()` methods.

- **Lightweight device enumeration (`find_devices_fast()`)** — new synchronous device
  discovery function that uses USB enumeration only, without opening serial ports or
  querying the device. Used by DFU wait loops to avoid 4+ second delays per poll cycle
  that caused the 10-second timeouts to overshoot.

Changed

- **DFU enter uses Attentio protocol** — `dfu-enter` and `dfu` (auto-enter) now send a
  raw AP DFU_ENTER packet on CDC1 (built via `build_packet()`) instead of the ChibiOS
  shell `dfu` text command. Required because the firmware no longer has a shell — CDC1 is
  now exclusively the AP interface.

- **Metadata command rewritten to use Attentio protocol** — `attentio metadata` now sends
  `GET_METADATA` (0x43) via the AP interface on CDC1 instead of the old shell text
  protocol. Displays all metadata fields (firmware version, build date, platform, etc.)
  in aligned table format or JSON.

- **Settings command rewritten to use Attntio protocol** — `attentio settings` now uses
  `SETTINGS_LIST` (0x50), `SETTINGS_GET` (0x51), and `SETTINGS_SET` (0x52) via the AP
  interface. All subcommands preserved: `list`, `get`, `set`, `save`, `load`.

- **Device name query during discovery uses AP protocol** — `find_devices()` now queries
  each normal-mode device's `device_name` setting via AP `SETTINGS_GET` on CDC1 to populate
  the `product` field, replacing the old shell-based `query_device_info()`.

- **CDC1 role renamed from "Shell" to "Protocol"** — `CdcRole::Shell` renamed to
  `CdcRole::Protocol` and `shell_port()` to `ap_port()` throughout the codebase to reflect
  the protocol change from ChibiOS text shell to AP interface.

Removed

- **Shell command (`attentio shell`)** — removed. The firmware no longer has a ChibiOS
  text shell; CDC1 is now the Attentio Protocol (AP) interface.
- **Send command (`attentio send`)** — removed. Relied on the ChibiOS shell text protocol
  which no longer exists.
- **TUI shell pane** — the TUI (`attentio tui`) now shows only the debug prints pane
  (CDC0) at full height. The interactive shell pane, input line, command history, and
  Tab-to-switch-panes keybinding have been removed.
- **Shell-related connection code** — removed `send_command()`, `sync_shell()`, and
  `drain_pending()` from `DeviceConnection`.

Fixed

- **DFU wait timeouts overshooting** — `wait_for_dfu_device()` and `wait_for_normal_device()`
  were calling `find_devices()` which opens serial ports and queries the shell, taking 4+
  seconds per poll cycle. With a 500ms poll interval and 10s timeout, the actual wait could
  exceed 40+ seconds before timing out. Both functions now use `find_devices_fast()` for
  sub-second polling.

## [Development] (2026-03-29)

Added

- **Device selection by index (`--device <#>`)** — The `--device` flag now accepts a
  1-based device number (matching the `#` column from `attentio list`) in addition to the
  full USB serial number. E.g., `attentio metadata --device 1` selects the first device.
  The index is computed statelessly from the serial-number-sorted device list — no
  persistent state or config files are involved.
- **Device index in `list` output** — `attentio list` now shows a `#` column as the first
  column. The `--json` output includes a corresponding `index` field per device.
- **`DeviceIndexOutOfRange` error** — when `--device <#>` specifies an index that exceeds
  the number of connected devices, the error message shows both the requested index and
  the actual device count, and suggests running `attentio list`.

Changed

- **`list` output redesigned as 2-line card layout** — each device now uses two lines.
  Line 1 (tabular): `#`, device name, device type, status, serial. Line 2 (indented):
  port paths and USB location with `Ports:` / `USB:` labels. Column order changed to put
  device name first for quick identification. Fits within ~85 columns instead of ~170+.
- **Simplified port role labels** — port roles in `list` output now show `[debug]`,
  `[shell]`, `[serial]` instead of `[CDC0 (debug_prints)]`, `[CDC1 (shell)]`, `[single]`.
- **Updated `MultipleDevices` error message** — now suggests `--device <#>` in addition
  to `--device <serial>`, and points users to `attentio list`.
- **Updated `--device` help text** — all subcommand help strings now describe the flag as
  accepting a serial number or index.

Added (previous)

- **Device identity from USB serial descriptor** — Device serial number is now read
  from the USB iSerialNumber descriptor (24-char chip UID) instead of querying the
  shell `metadata get serial_number` command. This works in both Normal and Bootloader
  (DFU) modes and eliminates the need for a shell connection to identify devices.
- **`rusb` fallback for serial number on Linux** — When `serialport` does not expose
  the USB serial string (common on Linux), the discovery module falls back to `rusb`
  direct USB enumeration to read the iSerialNumber descriptor.
- **DFU serial filtering** — All DFU operations (`dfu`, `dfu-enter`, `bootloader-enter`)
  now filter by USB serial number to target the correct device when multiple devices are
  connected. New `open_dfu_by_serial()` helper opens a specific DFU device by serial.
- **Pure DFU device serial numbers** — `find_dfu_only_devices()` now reads the USB
  serial descriptor via `rusb`, so devices in bootloader mode display their serial in
  `attentio list`.
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

- **`query_device_info()` simplified** — No longer queries `metadata get serial_number`
  from the device shell. Serial number comes from USB descriptor; only `device_name` is
  still queried from shell settings.
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

