# Changelog

All notable changes to the Attentio CLI (`attentio`) project will be documented in this file.

**Version Format:** MAJOR.MINOR.PATCH
- **MAJOR:** Incompatible API/protocol changes
- **MINOR:** New features (backward compatible)
- **PATCH:** Bug fixes (backward compatible)

[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Note: Update `Cargo.toml` when publishing new version.

---

## [Development] (2026-06-11)

Added

- **BLE transport (`--ble`)** — the CLI can now drive a device over Bluetooth Low
  Energy instead of USB-CDC, speaking the same Attentio Protocol (AP) the device
  already serves over its GATT service. USB remains the default; passing `--ble`
  routes the connection through the new BLE transport. The AP wire layer
  (`protocol/packet.rs`, `protocol/crc.rs`) is unchanged and transport-agnostic.

  - New `src/device/ble.rs` (built on `btleplug`): scans by the Attentio service
    UUID (`1209eea1-0001-…`), connects, resolves the TX (`…-0002`) / RX (`…-0003`)
    characteristics, subscribes to RX notifications (pumped into the reader via an
    mpsc channel), and chunks TX writes to MTU−3 (the device reassembles by AP
    `LEN`). GATT setup is bounded by explicit connect / service-resolve / subscribe
    timeouts so a stalled BlueZ D-Bus call fails fast with a clear message.
  - **Device selector** — `--ble` (bare) connects to the single advertised
    `AttentioLight-1`; `--ble=<name>`, `--ble=<MAC>`, or `--ble=<N>` (the `#` from
    `attentio list`) pin a specific device. Wired as a `BleSelector` set once in
    `main.rs` and read by the open-path.
  - **Unified `attentio list`** — now enumerates USB **and** BLE devices with a
    per-row transport tag and a `paired: yes/no` indicator. `AttentioDevice`
    carries `transport: Usb | Ble` and `ble_address`; `device/discovery.rs` reuses
    `ble::scan` / `ble::paired_status`.
  - **`attentio --ble monitor`** — the AP monitor view works over BLE as well as
    USB.
  - **Linux bonding + bond auto-heal** — the TX characteristic is
    encryption-required, so the CLI bonds before the first write. `btleplug` 0.11
    exposes no `pair()`, so on Linux this is done via `bluetoothctl` (no
    auto-`trust`, which would let BlueZ background-connect and trip the device's
    single-session firmware). If a host that is already bonded fails to connect,
    discover, or subscribe with the stale-LTK signature, the CLI drops the host
    key (`bluetoothctl remove`), re-scans, re-pairs, and retries the connect once
    — healing the common "host has an LTK the device no longer holds" case.

- **New error variants** — `AttentioError::Ble`, `BleNotFound { selector }`, and
  `BlePairing` for BLE-specific failures, with JSON `error_type` / `context_data`
  support.

- **New dependencies** — `btleplug` 0.11 (cross-platform BLE), `futures-util`
  (notification stream), and `uuid`. Under `-v`, `btleplug` / `bluez_async` logs
  are surfaced to diagnose stalled GATT operations.

Changed

- **`ConnReader` / `ConnWriter` / `ConnGuard` gain `Ble` variants**
  (`device/connection.rs`) alongside the existing `Serial` path, so
  `ApClient::from_parts` drives a BLE connection exactly like a serial one. The
  serial transport and USB flows are unchanged.

---

## [Development] (2026-05-16)

Added

- **CDC role probing for Windows** — `find_devices()` now probes dual-CDC devices
  to determine which serial port is the Attentio Protocol port (CDC1) and which
  is the serial prints port (CDC0). On Linux, `/dev/ttyACM0` is always
  interface 0 and `/dev/ttyACM1` is interface 1, so alphabetical port ordering
  works. On Windows, COM port numbers are assigned arbitrarily by the OS and
  don't correspond to USB interface order. The probe sends an AP PING command
  to each candidate port and swaps CDC0/CDC1 if the default assignment is
  wrong. A successful probe result is cached per device serial so subsequent
  discovery cycles don't need to re-probe (which would fail when the port is
  already held exclusively by ApClient).

  Three new functions in `device::discovery`:
  - `probe_and_fix_cdc_roles()` — async function called from `find_devices()`
    before `query_device_name()`. Checks the cache first, then probes both
    ports if needed.
  - `probe_port_ping()` — opens a port, sends AP PING, returns true if the
    device responds. Uses a 500ms timeout.
  - `cdc_protocol_cache_remember()` / `cdc_protocol_cache_lookup()` —
    process-local cache mapping device serial → known protocol port path,
    so repeated discovery cycles skip probing when the ApClient already
    holds the port.

Fixed

- **AP commands timing out on Windows** — on some Windows configurations,
  COM port numbers don't match USB interface order (e.g., COM3 is the
  protocol port and COM4 is the serial prints port). The previous code
  always assigned the lower-numbered port as CDC0 (serial prints) and the
  higher-numbered as CDC1 (protocol), which was wrong for these devices.
  AP commands sent to the wrong port received no response and timed out
  after 3 seconds. The probe correctly identifies the protocol port on
  first discovery and caches the result for the app's lifetime.

---

## [Development] (2026-06-03)

Changed

- `src/device/connection.rs`, `ConnReader` / `ConnWriter` are now single-variant
  enums (`Serial`) that dispatch `read_raw` / `write_raw`, establishing the seam
  for a future BLE transport variant. Behavior-preserving: `ApClient`, the reader
  loop, and the AP/CRC layer are untouched, and USB serial remains the only
  transport. Builds clean; all unit tests pass.

---

## [Development] (2026-05-14)

Added

- **Windows build support** — the crate now compiles for `x86_64-pc-windows-gnu`
  and `x86_64-pc-windows-msvc` targets. Previously the device I/O layer used
  POSIX-only APIs unconditionally (`libc::open`, `termios`, `TIOCEXCL`,
  `/proc/*/fd` scanning), so the crate could only be built on Unix. The Linux
  code path is unchanged.

  - `device::connection`: the existing Unix `open_serial()` (raw `libc::open` →
    `TIOCEXCL` → `cfmakeraw` → `serialport::TTYPort::from_raw_fd` →
    `tokio_serial::SerialStream`) is now gated behind `#[cfg(unix)]`. A new
    `#[cfg(windows)]` branch opens the port via
    `tokio_serial::new(path, 115_200).open_native_async()`, which uses
    `CreateFileW` without sharing flags — giving exclusive access for free.
    `serialport::Error` of kind `PermissionDenied` is mapped to
    `AttentioError::PortBusy` so callers see the same error type on both
    platforms.
  - `DeviceConnection.fd` / `owns_fd` and the `FdGuard.fd` field are
    `#[cfg(unix)]`. On Windows `FdGuard` is a unit marker; the `Drop` impls do
    nothing on Windows because the OS releases the port handle on close.
  - `device::discovery`: `CdcPort.path` doc updated to mention Windows `COM*`
    paths alongside `/dev/ttyACM0`. `resolve_port_serial` keeps its Linux-only
    sysfs fallback; on Windows `serialport`'s `serial_number` field is
    populated reliably.
  - `cli::commands::dfu`: three "check USB permissions (udev rules)" error
    hints replaced by a single `usb_permission_hint()` helper that returns
    platform-appropriate text — udev guidance on Linux, "install a WinUSB
    driver for the DFU interface (use Zadig)" on Windows.

  Verified with `cargo check` for both the host Linux target and
  `x86_64-pc-windows-gnu` (clean, no warnings). DFU over USB on Windows still
  requires the user to bind a WinUSB driver to the bootloader interface
  manually via Zadig.

---

## [Development] (2026-05-10)

Added

- **`DfuEvent` and `flash_firmware_for_serial()` — GUI-friendly DFU library API** —
  added to the crate's public surface (`src/cli/commands/dfu`) so the desktop app
  can drive firmware updates programmatically without terminal output or interactive
  device selection.

  `DfuEvent` carries structured progress variants (`ValidatingFirmware`,
  `EnteringBootloader`, `Erasing`, `Writing { bytes_written, bytes_total }`,
  `WaitingForReboot`, `Done`) delivered through a
  `tokio::sync::mpsc::UnboundedSender` whose `send()` is sync and therefore safe to
  call from `spawn_blocking` threads.

  `flash_firmware_for_serial(serial, firmware_data, tx)` reuses all existing private
  helpers (`FirmwareHeader::parse/validate`, `execute_enter_internal`,
  `open_dfu_by_serial`, `wait_for_device_mode`) and adds a new blocking
  `flash_dfu_device_with_events` variant that reports byte-level write progress
  through the channel instead of `indicatif` terminal bars. The CLI binary and its
  commands are unaffected.

---

## [Development] (2026-05-09)

Added

- **Permanent CDC1 reader task in `ApClient`** — `ApClient::new` now spawns a
  background reader task that owns the read half of the AP serial connection
  and runs an `ApParser` continuously. Parsed `ApResponse` frames are
  dispatched as follows:
  - **Events** (cmd in `EVENT_CMD_RANGE`, 0x80–0x8F) are broadcast on
    `monitor_tx` only — they never reach `send_command` waiters.
  - **Command responses** (everything else) are delivered to the in-flight
    command waiter via a oneshot channel and also broadcast for monitor views.

  This fixes the long-standing race where an `EVT_BUTTON` arriving between a
  CLI command write and the response read would either be misinterpreted as
  the response (causing "unexpected response command" errors in the desktop
  app) or stall the next command. Public `ApClient` API is unchanged;
  `drain()` is kept for compatibility but is now a brief settle delay no-op
  (the reader is always draining).

- **`DeviceConnection::into_parts()`** — splits a connection into a
  `ConnReader`, `ConnWriter`, and `FdGuard`. The `FdGuard` takes over the
  responsibility of clearing `TIOCEXCL` on drop (`DeviceConnection`'s own
  `Drop` is disarmed via an `owns_fd` flag), so the read and write halves can
  be moved into independent tasks while the port is still released cleanly
  when the last guard drops. Used by `ApClient` to give the reader task
  exclusive ownership of the read half.

- **`CmdClass` enum and `EVENT_CMD_RANGE` / `REQUEST_CMD_RANGE` constants in
  `protocol::packet`** — single source of truth for AP command-byte
  classification, mirroring the firmware's documented layout (0x00–0x7F
  request, 0x80–0x8F event, 0xF0/0xF1 response). New `ApResponse::is_event()`
  method built on top. Replaces the magic literal `matches!(cmd, 0x80..=0x8F)`
  that lived in `client.rs`. Adding new event ids in 0x80–0x8F or new
  requests in 0x00–0x7F now requires zero classification-code changes. Five
  unit tests cover known commands, range boundaries, and consistency.

Changed

- **DFU device selection now goes through `select_device`** — `attentio dfu
  <firmware.bin>` previously had its own ad-hoc target-resolution logic
  (auto-pick a lone bootloader; fall back to "first VID/PID match" inside
  `dfu-libusb`). It now resolves the target up-front via the same
  `select_device(devices, --device)` helper used by every other command,
  guaranteeing a concrete USB serial before any `rusb`/`dfu-libusb` call and
  removing the unfiltered `DfuLibusb::open(VID, PID, ...)` fallback that could
  grab the wrong board when multiple devices were connected. Added an
  explicit "unknown serial" guard and clearer per-mode messages
  (Bootloader / Normal / Unknown).

- **`flash_dfu_device`, `flash_dfu_device_inner`, `reset_dfu_device`,
  `wait_for_dfu_device_sync`, `find_matching_attentio_usb_device`,
  `open_dfu_by_serial`** — all six DFU helpers now take `serial: &str`
  instead of `Option<&str>`. The "no serial → match anything" path has been
  removed entirely.

- **DFU diagnostics** — `find_matching_attentio_usb_device` now logs counts
  of VID/PID matches, `device.open()` failures, and serial-read failures at
  `debug` level, making it easier to diagnose udev / permission problems
  when the target serial cannot be reached.

Fixed

- **Mid-command `EVT_BUTTON` no longer corrupts command responses** — when
  the device sent an `EVT_BUTTON` (0x80) or other asynchronous event while
  the CLI/desktop client was waiting for a command response,
  `send_command_ok` returned an "unexpected response command" error
  ("Fail to post message to Dart" in the desktop app). Events are now
  filtered out by the permanent reader task and routed to the monitor
  broadcast only; command waiters always see a real `OK` / `ERROR` response.

---

## [Development] (2026-05-07)

Added

- **AP client monitor broadcast channel** — `ApClient` now owns a
  `tokio::sync::broadcast::Sender<MonitorEvent>` and sends two event types:
  `Outgoing { cmd, payload }` when a command is sent and `Incoming(ApResponse)`
  when a response (or event) is received. The channel is lazily created (256
  capacity) and shared with monitor consumers via `subscribe_monitor()`, which
  returns a `broadcast::Receiver`. Enables `attentio-desktop`'s Monitor page to
  tap into the same `ApClient` instance used by all device commands.

Changed

- **Centralised `log_level_name()`** — the human-readable log level name
  function (NONE / ERROR / WARN / INFO / DEBUG) has been moved from
  `cli/commands/loglevel.rs` into `monitor/format.rs` so it can be shared by
  both the CLI and `attentio-desktop`'s Rust bridge. `loglevel.rs` now
  delegates to `monitor::format::log_level_name()`.

---

## [Development] (2026-05-04)

Changed

- **Generalized crate description** — `Cargo.toml` description updated from
  "CLI tool for AttentioLight-1 (AL-1) device management" to "CLI tool for
  Attentio device management" to reflect broader product naming.

---

## [Development] (2026-05-03)

Added

- **Interactive device picker** — when multiple devices are connected and no
  `--device` flag is given, the CLI now prompts the user to select a device
  from a numbered list (showing index, serial number, and device name) instead
  of printing an error. Includes a cancel option. Falls back to the previous
  error message in non-interactive (piped/scripted) contexts.

Fixed

- **Ping round-trip timing** — `attentio ping` no longer includes device
  discovery and selection time in the reported round-trip duration. The timer
  now starts after the device connection is established.

---

## [Development] (2026-05-03)

Fixed

- **Multi-device discovery on Linux** — fixed a bug where multiple connected
  AttentioLight-1 devices were merged into a single entry in `attentio list`.
  The `serialport` crate often returns `serial_number: None` on Linux, causing
  all ports to be grouped under `"unknown"` in a single device. Added
  `read_serial_from_sysfs()` fallback that reads the USB serial number from
  `/sys/class/tty/<tty>/device/../serial`, which the kernel always populates
  from the device's iSerialNumber descriptor. Each device now correctly
  appears as a separate entry with its unique 24-char hex serial.

- **Flaky device name reads** — `attentio list` intermittently showed `-` for
  device names because the AP protocol query failed on the first attempt
  (stale bytes in the CDC receive buffer or insufficient settle time after
  enumeration). Three improvements:
  - **Parallel queries** — device name queries now run concurrently via
    `tokio::task::JoinSet` instead of sequentially, reducing total latency.
  - **Stale byte drain** — new `ApClient::drain()` method reads and discards
    leftover bytes from the CDC receive buffer before sending the AP command.
  - **Retry with backoff** — on first failure, retries once after a 150ms
    backoff delay. Settle delay increased from 50ms to 100ms.

---

## [Development] (2026-05-01)

Changed

- **`cache_remember` made public** — `device::discovery::cache_remember` is
  now `pub` so external consumers (e.g. `attentio-desktop`) can update the
  last-known device-name cache after a rename without waiting for the next
  discovery poll.

---

## [Development] (2026-04-26)

Added

- **Last-known device-name cache** — `device::discovery` now keeps a
  process-local `OnceLock<Mutex<HashMap<String, String>>>` mapping USB
  serial → most recently observed `device_name` setting. `find_devices`
  updates the entry on every successful AP read of `device_name` and falls
  back to the cached value when the read fails (e.g. the AP port is
  momentarily busy). Consumers (CLI `list`, `attentio-desktop`, future
  bindings) thus see a stable name across transient enumeration blips
  rather than `None` followed by a fallback for one tick.

  No public API change; the cache lives entirely in the library and self-
  corrects on the next successful read.

---

## [Development] (2026-04-24)

Added

- **Library crate exposure (`[lib]`)** — `attentio-cli` now builds as both a
  binary and a library. Added an explicit `[lib]` section to `Cargo.toml`
  (`name = "attentio"`, `path = "src/lib.rs"`) so that external Rust crates
  can depend on the CLI's protocol, device, monitor, and error modules as a
  library. This is consumed by `attentio-desktop`'s
  `rust_lib_attentio_desktop` to reuse `libattentio` directly instead of
  duplicating protocol code.

Changed

- **`main.rs` imports** — the binary now uses `use attentio::cli;` and
  `use attentio::json_output;` (two extern-crate imports) instead of
  `mod cli; mod device; ...` since the modules live in the library crate.
  Unused extern imports for `device`, `error`, `monitor`, and `protocol`
  were removed — those modules are still accessed transitively through
  `cli::commands::*`.

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

