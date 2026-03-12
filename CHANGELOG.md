# Changelog

All notable changes to the Attentio CLI (`attentio`) project will be documented in this file.

**Version Format:** MAJOR.MINOR.PATCH
- **MAJOR:** Incompatible API/protocol changes
- **MINOR:** New features (backward compatible)
- **PATCH:** Bug fixes (backward compatible)

[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Note: Update `Cargo.toml` when publishing new version.

---

## [Development] (2026-03-12)

Added

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

Changed

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
- Improved TUI terminal setup/cleanup — raw mode and alternate screen init wrapped so
  cleanup always runs even if setup fails.
- Commented out unused port mappings in `.devcontainer/docker-compose.yml`.
- Removed `#[allow(dead_code)]` from `debug_port()`, `read_line()`, and `with_timeout()` — now used by TUI.
- Updated README implementation status: TUI command marked as Done.

---

## [Development] (2026-03-03)

Added

- **CLI framework** with clap (derive) supporting subcommands: `list`, `send`, `shell`, `monitor`, `led`, `settings`, `dfu`, `dfu-enter`, and `completions`.
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

Changed

- Expanded README from placeholder to full documentation.
- Updated `docs/PLAN.md` with future items (unit tests, CI/CD).

Fixed

- Fixed typo in udev script filename (`udev_rules_attetio.sh` → `udev_rules_attentio.sh`).
