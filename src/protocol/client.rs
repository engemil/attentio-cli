//! High-level AP protocol client.
//!
//! Wraps a [`DeviceConnection`] and provides typed methods for each AP command.
//! The protocol is mostly synchronous: send one command, receive one response.
//! Unsolicited device-to-host events (see [`EVENT_CMD_RANGE`]) are also
//! possible — those are routed to the monitor broadcast channel rather than
//! to a command waiter.
//!
//! ## Architecture
//!
//! On construction, [`ApClient`] spawns a permanent background reader task
//! that owns the read half of the underlying serial connection. The task runs
//! an [`ApParser`] continuously and dispatches each parsed [`ApResponse`]:
//!
//! - **Events** ([`CmdClass::Event`]): broadcast on `monitor_tx` only. Multiple
//!   subscribers (CLI/desktop monitor view) receive each event.
//! - **Command responses** (everything else): delivered to the in-flight
//!   `send_command` waiter via a oneshot channel, AND broadcast on `monitor_tx`
//!   so monitor views see the response too.
//!
//! [`send_command`](ApClient::send_command) acquires a per-client mutex (so
//! commands serialize on the wire), registers a oneshot waiter, writes the
//! packet, and awaits the oneshot with a timeout. The reader task is the only
//! place that reads from the serial port.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::{broadcast, oneshot, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::device::connection::{ConnGuard, ConnReader, ConnWriter, DeviceConnection};
use crate::device::discovery::resolve_device;
use crate::error::AttentioError;

use super::packet::{
    ap_error_name, build_packet, ApParser, ApResponse, AP_ERR_NOT_CONTROLLER, CMD_CLAIM,
    CMD_GET_METADATA, CMD_GET_STATUS, CMD_LED_OFF, CMD_LOG_GET_LEVEL, CMD_LOG_SET_LEVEL,
    CMD_METADATA_GET, CMD_PING, CMD_POWER_OFF, CMD_POWER_ON, CMD_RELEASE, CMD_SETTINGS_GET,
    CMD_SETTINGS_LIST, CMD_SETTINGS_SET, CMD_SET_BRIGHTNESS, CMD_SET_HSV, CMD_SET_RGB,
};

/// Events emitted by the AP client monitor tap.
#[derive(Debug, Clone)]
pub enum MonitorEvent {
    /// A command was sent from host to device.
    Outgoing { cmd: u8, payload: Vec<u8> },
    /// A response/event was received from device.
    Incoming(ApResponse),
}

/// Default timeout for AP command responses.
const AP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

/// Parsed device status from GET_STATUS response.
#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub system_state: u8,
    pub current_r: u8,
    pub current_g: u8,
    pub current_b: u8,
    pub brightness: u8,
    pub control_mode: u8,
    pub active_controller: u8,
    pub standalone_mode: u8,
    pub effects_submode: u8,
    pub standalone_color_index: u8,
    pub standalone_brightness_raw: u8,
    pub anim_type: u8,
    pub session_id: u16,
}

/// Human-readable name for control mode byte.
pub fn control_mode_name(mode: u8) -> &'static str {
    match mode {
        0 => "STANDALONE",
        1 => "REMOTE",
        _ => "UNKNOWN",
    }
}

/// Human-readable name for interface ID byte.
pub fn interface_name(id: u8) -> &'static str {
    match id {
        0 => "NONE",
        1 => "STANDALONE",
        2 => "USB",
        3 => "BLE",
        4 => "WiFi",
        _ => "UNKNOWN",
    }
}

/// Human-readable name for system state byte.
pub fn system_state_name(state: u8) -> &'static str {
    match state {
        0 => "BOOT",
        1 => "POWERUP",
        2 => "ACTIVE",
        3 => "POWERDOWN",
        4 => "OFF",
        _ => "UNKNOWN",
    }
}

/// Human-readable name for standalone mode byte.
pub fn standalone_mode_name(mode: u8) -> &'static str {
    match mode {
        0 => "Solid Color",
        1 => "Brightness",
        2 => "Blinking",
        3 => "Pulsation",
        4 => "Effects",
        5 => "Traffic Light",
        6 => "Night Light",
        _ => "UNKNOWN",
    }
}

/// Human-readable name for effects sub-mode byte.
pub fn effects_submode_name(submode: u8) -> &'static str {
    match submode {
        0 => "Rainbow",
        1 => "Color Cycle",
        2 => "Breathing",
        3 => "Candle",
        4 => "Fire",
        5 => "Lava Lamp",
        6 => "Day/Night",
        7 => "Ocean",
        8 => "Northern Lights",
        9 => "Thunder Storm",
        10 => "Police",
        11 => "Health Pulse",
        12 => "Memory",
        _ => "UNKNOWN",
    }
}

/// Resolve a device and open an AP client connection.
///
/// This is the standard pattern used by all command handlers that need to
/// communicate with a device: resolve by serial/index, find the AP port,
/// wait for CDC ACM to settle, and open the protocol client.
pub async fn open_client(target: Option<&str>) -> Result<ApClient> {
    // If --ble was given, route to the BLE transport. The serial `target`
    // (serial/index) does not apply; the BLE selector carries name/address.
    if let Some(selector) = crate::device::ble::active_selector() {
        let (reader, writer, guard) = crate::device::ble::open(&selector)
            .await
            .context("failed to open BLE connection")?;
        return Ok(ApClient::from_parts(reader, writer, guard));
    }

    let dev = resolve_device(target)
        .await
        .context("failed to resolve device")?;

    open_client_for_device(&dev).await
}

/// Open an AP client connection to an already-resolved device.
///
/// Use this when you already have a resolved [`AttentioDevice`] and want to
/// avoid a redundant device enumeration.
pub async fn open_client_for_device(
    dev: &crate::device::discovery::AttentioDevice,
) -> Result<ApClient> {
    let port_path = dev
        .ap_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no protocol port", dev.serial))?
        .to_string();

    // Brief delay to let CDC ACM link settle after enumeration.
    tokio::time::sleep(Duration::from_millis(50)).await;

    ApClient::open(&port_path)
        .with_context(|| format!("failed to open protocol port {}", port_path))
}

/// AP protocol client wrapping a serial connection.
///
/// Internally owns:
/// - A permanent reader task that parses incoming bytes from CDC1.
/// - A writer (mutexed) for outgoing command packets.
/// - A registry of in-flight command waiters (oneshot channels).
/// - A broadcast tap so monitor views can observe all traffic.
pub struct ApClient {
    /// Serializes command writes and waiter registration.
    writer: Arc<Mutex<ConnWriter>>,
    /// Holds the oneshot sender of the currently in-flight command, if any.
    /// Used by the reader task to deliver a response to the awaiter.
    /// Wrapped in `std::sync::Mutex` (sync) because it's only briefly held to
    /// take/place a `Sender` — never across `.await`.
    pending: Arc<std::sync::Mutex<Option<oneshot::Sender<ApResponse>>>>,
    /// Broadcast channel for monitor tap.
    monitor_tx: broadcast::Sender<MonitorEvent>,
    /// Per-client command timeout.
    timeout: Duration,
    /// Whether `claim()` has succeeded in this session.
    claimed: bool,
    /// Reader task handle — aborted on drop.
    reader_task: Option<JoinHandle<()>>,
    /// Holds the transport resources (serial fd / BLE peripheral) for release on
    /// drop. Must outlive the reader task and the writer.
    _guard: Arc<ConnGuard>,
}

impl Drop for ApClient {
    fn drop(&mut self) {
        if let Some(handle) = self.reader_task.take() {
            handle.abort();
        }
    }
}

impl ApClient {
    /// Create a new AP client from already-split transport parts.
    ///
    /// Transport-agnostic — both the serial path ([`Self::new`]) and the BLE
    /// path ([`crate::device::ble::open`]) funnel through here. Spawns the
    /// permanent reader task and returns immediately.
    pub fn from_parts(reader: ConnReader, writer: ConnWriter, guard: ConnGuard) -> Self {
        let (monitor_tx, _) = broadcast::channel(256);
        let pending: Arc<std::sync::Mutex<Option<oneshot::Sender<ApResponse>>>> =
            Arc::new(std::sync::Mutex::new(None));
        let guard = Arc::new(guard);

        let reader_task = tokio::spawn(reader_loop(
            reader,
            monitor_tx.clone(),
            pending.clone(),
            // Hold a clone of the guard inside the task so the transport is not
            // released until the task itself drops.
            guard.clone(),
        ));

        Self {
            writer: Arc::new(Mutex::new(writer)),
            pending,
            monitor_tx,
            timeout: AP_RESPONSE_TIMEOUT,
            claimed: false,
            reader_task: Some(reader_task),
            _guard: guard,
        }
    }

    /// Create a new AP client from an open serial connection.
    ///
    /// Spawns the permanent reader task and returns immediately.
    pub fn new(conn: DeviceConnection) -> Self {
        let (reader, writer, guard) = conn.into_parts();
        Self::from_parts(reader, writer, guard)
    }

    /// Create a new AP client, opening a connection to the given port.
    pub fn open(port_path: &str) -> Result<Self, AttentioError> {
        let conn = DeviceConnection::open(port_path)?;
        Ok(Self::new(conn))
    }

    /// Live signal strength (RSSI, dBm) of the transport, or `None` for serial
    /// connections (and BLE links whose RSSI the adapter hasn't surfaced yet).
    pub async fn ble_rssi(&self) -> Option<i16> {
        self._guard.ble_rssi().await
    }

    /// Drain any stale bytes from the receive buffer.
    ///
    /// With the permanent reader architecture, "draining" is implicit: the
    /// reader task is already consuming bytes continuously and routing parsed
    /// frames either to the broadcast (for events) or to nothing (when no
    /// command is in flight, command-shaped responses just get logged and
    /// dropped). This method is kept for API compatibility but is now a no-op
    /// after a brief settle delay.
    pub async fn drain(&mut self) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    /// Set the response timeout.
    #[allow(dead_code)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Subscribe to a stream of monitor events (outgoing commands and incoming
    /// responses/events). Events are best-effort — if the receiver falls
    /// behind, older events are dropped.
    pub fn subscribe_monitor(&self) -> broadcast::Receiver<MonitorEvent> {
        self.monitor_tx.subscribe()
    }

    /// Send an AP command and wait for the response.
    ///
    /// Builds the packet, registers a one-shot waiter, writes the packet, and
    /// awaits delivery from the permanent reader task. Unsolicited events
    /// (cmd 0x80–0x8F) are filtered out in the reader and never delivered to
    /// command waiters, so this method always returns a command-shaped
    /// response (or a timeout / IO error).
    pub async fn send_command(
        &mut self,
        cmd: u8,
        payload: &[u8],
    ) -> Result<ApResponse, AttentioError> {
        let pkt = build_packet(cmd, payload);
        debug!(
            "AP send: cmd=0x{:02X} payload_len={} pkt_len={}",
            cmd,
            payload.len(),
            pkt.len()
        );

        // Register the oneshot waiter BEFORE writing, so we can't miss a
        // very-fast response. Hold the writer mutex throughout to serialize
        // commands on the wire.
        let mut writer = self.writer.lock().await;

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .expect("ApClient pending mutex poisoned");
            // If a previous waiter was somehow left in place (shouldn't happen
            // because writer mutex serializes us), drop it — its caller already
            // returned with a timeout.
            *pending = Some(tx);
        }

        // Best-effort outgoing broadcast (after registration so the order in
        // monitor matches the wire order).
        let _ = self.monitor_tx.send(MonitorEvent::Outgoing {
            cmd,
            payload: payload.to_vec(),
        });

        if let Err(e) = writer.write_raw(&pkt).await {
            // Clean up the waiter we just registered.
            let mut pending = self
                .pending
                .lock()
                .expect("ApClient pending mutex poisoned");
            *pending = None;
            return Err(e);
        }

        // Hold the writer lock until we've awaited the response so that no
        // other command can interleave its waiter registration. (Commands
        // are strictly request/response per the AP protocol.)
        let result = tokio::time::timeout(self.timeout, rx).await;
        drop(writer);

        match result {
            Ok(Ok(resp)) => {
                debug!(
                    "AP recv: cmd=0x{:02X} payload_len={}",
                    resp.cmd,
                    resp.payload.len()
                );
                Ok(resp)
            }
            Ok(Err(_canceled)) => {
                // Sender dropped without sending — reader task likely exited.
                Err(AttentioError::Protocol {
                    message: "connection closed while waiting for AP response".to_string(),
                })
            }
            Err(_elapsed) => {
                // Timeout — clear the waiter so the reader doesn't try to
                // deliver into a dead oneshot.
                let mut pending = self
                    .pending
                    .lock()
                    .expect("ApClient pending mutex poisoned");
                *pending = None;
                Err(AttentioError::Timeout {
                    seconds: self.timeout.as_secs(),
                })
            }
        }
    }

    /// Send a command and return the payload on success, or an error with a
    /// human-readable message on failure.
    ///
    /// Note: with the permanent reader architecture, unsolicited events
    /// (cmd 0x80–0x8F) are filtered out before reaching `send_command`, so
    /// this function will never observe an event response.
    async fn send_command_ok(&mut self, cmd: u8, payload: &[u8]) -> Result<Vec<u8>, AttentioError> {
        Self::interpret_ok(self.send_command(cmd, payload).await?)
    }

    /// Map a parsed AP response to its payload on success, or a descriptive
    /// [`AttentioError::Protocol`] on an error/event/unexpected response.
    ///
    /// Split out of [`send_command_ok`] so [`send_claimed`] can inspect the raw
    /// response (to detect a takeover) before applying the same mapping.
    fn interpret_ok(resp: ApResponse) -> Result<Vec<u8>, AttentioError> {
        if resp.is_ok() {
            Ok(resp.payload)
        } else if resp.is_error() {
            let code = resp.error_code().unwrap_or(0xFF);
            Err(AttentioError::Protocol {
                message: format!(
                    "device returned error 0x{:02X}: {}",
                    code,
                    ap_error_name(code)
                ),
            })
        } else if resp.is_event() {
            // Should never happen — the reader task routes events to the
            // broadcast and never to command waiters. Treat as a protocol bug.
            Err(AttentioError::Protocol {
                message: format!(
                    "internal error: unsolicited event 0x{:02X} delivered to command waiter",
                    resp.cmd
                ),
            })
        } else {
            Err(AttentioError::Protocol {
                message: format!("unexpected response command: 0x{:02X}", resp.cmd),
            })
        }
    }

    /// Send a claim-requiring command, auto-claiming first and transparently
    /// re-claiming (taking over) + retrying once if the device reports we are no
    /// longer the active controller (`AP_ERR_NOT_CONTROLLER`).
    ///
    /// The firmware uses a last-writer-wins takeover model: a CLAIM from another
    /// interface (e.g. switching between this device's USB and BLE links) silently
    /// displaces us and is signalled only by an out-of-band SESSION_END event,
    /// which this command-only client never observes. As a result our cached
    /// `claimed` flag can go stale — `ensure_claimed` then skips the re-CLAIM and
    /// the command is rejected with `AP_ERR_NOT_CONTROLLER`. Healing it here (one
    /// bounded retry) keeps that error from surfacing to the caller.
    async fn send_claimed(&mut self, cmd: u8, payload: &[u8]) -> Result<Vec<u8>, AttentioError> {
        self.ensure_claimed().await?;
        let resp = self.send_command(cmd, payload).await?;
        if resp.is_error() && resp.error_code() == Some(AP_ERR_NOT_CONTROLLER) {
            // Stale claim — another interface took over. Re-claim (takeover) and
            // retry the command exactly once.
            self.claimed = false;
            self.claim().await?;
            return Self::interpret_ok(self.send_command(cmd, payload).await?);
        }
        Self::interpret_ok(resp)
    }

    // ── Session commands ─────────────────────────────────────────────────────

    /// Claim control of the device (CLAIM 0x01).
    ///
    /// Transitions device from STANDALONE to REMOTE mode (or takes over from
    /// another controller). Returns the session ID assigned by the device.
    /// After a successful claim, the client can issue commands that require
    /// claim (LED, power, settings set).
    pub async fn claim(&mut self) -> Result<u16, AttentioError> {
        let payload = self.send_command_ok(CMD_CLAIM, &[]).await?;
        self.claimed = true;
        let session_id = if payload.len() >= 2 {
            u16::from_be_bytes([payload[0], payload[1]])
        } else {
            0
        };
        Ok(session_id)
    }

    /// Release control of the device (RELEASE 0x02).
    ///
    /// Returns device to STANDALONE mode.
    pub async fn release(&mut self) -> Result<(), AttentioError> {
        self.send_command_ok(CMD_RELEASE, &[]).await?;
        self.claimed = false;
        Ok(())
    }

    /// Ping the device (PING 0x03).
    pub async fn ping(&mut self) -> Result<(), AttentioError> {
        self.send_command_ok(CMD_PING, &[]).await?;
        Ok(())
    }

    /// Ensure the device is claimed before issuing a claim-required command.
    ///
    /// Sends CLAIM if not already claimed in this session. The claim is kept
    /// active until explicitly released.
    pub async fn ensure_claimed(&mut self) -> Result<(), AttentioError> {
        if !self.claimed {
            self.claim().await?;
        }
        Ok(())
    }

    // ── Query commands ───────────────────────────────────────────────────────

    /// Query device metadata (GET_METADATA 0x43) with pagination.
    ///
    /// Fetches all pages and returns the full list of (key, value) pairs.
    /// Response payload per page:
    /// `[total_count:1][page:1][page_count:1] { [key_len:1][key][val_len:1][val] } * page_count`
    pub async fn get_metadata(&mut self) -> Result<Vec<(String, String)>, AttentioError> {
        let mut all_entries = Vec::new();
        let mut page: u8 = 0;

        loop {
            let payload = self.send_command_ok(CMD_GET_METADATA, &[page]).await?;
            let (total, _current_page, entries) = parse_kv_paginated(&payload)?;
            all_entries.extend(entries);

            if all_entries.len() >= total as usize {
                break;
            }
            page += 1;
        }

        Ok(all_entries)
    }

    /// Get a single metadata field by key (METADATA_GET 0x44).
    ///
    /// Request payload: `[key_len:1][key]`
    /// Response payload: `[key_len:1][key][val_len:1][val]` (single pair, no count).
    pub async fn get_metadata_field(
        &mut self,
        key: &str,
    ) -> Result<(String, String), AttentioError> {
        let mut req = Vec::with_capacity(1 + key.len());
        req.push(key.len() as u8);
        req.extend_from_slice(key.as_bytes());

        let payload = self.send_command_ok(CMD_METADATA_GET, &req).await?;
        parse_kv_single(&payload)
    }

    /// Query device status (GET_STATUS 0x40).
    ///
    /// Returns the current device status including RGB, brightness, mode, etc.
    /// Supports both 8-byte (legacy firmware) and 12-byte (v2) responses.
    pub async fn get_status(&mut self) -> Result<DeviceStatus, AttentioError> {
        let payload = self.send_command_ok(CMD_GET_STATUS, &[]).await?;
        if payload.len() < 8 {
            return Err(AttentioError::Protocol {
                message: format!(
                    "GET_STATUS response too short: {} bytes (expected >= 8)",
                    payload.len()
                ),
            });
        }
        Ok(DeviceStatus {
            system_state: payload[0],
            current_r: payload[1],
            current_g: payload[2],
            current_b: payload[3],
            brightness: payload[4],
            control_mode: payload[5],
            active_controller: payload[6],
            standalone_mode: payload[7],
            effects_submode: payload.get(8).copied().unwrap_or(0),
            standalone_color_index: payload.get(9).copied().unwrap_or(0),
            standalone_brightness_raw: payload.get(10).copied().unwrap_or(0),
            anim_type: payload.get(11).copied().unwrap_or(0),
            session_id: {
                let hi = payload.get(12).copied().unwrap_or(0) as u16;
                let lo = payload.get(13).copied().unwrap_or(0) as u16;
                (hi << 8) | lo
            },
        })
    }

    // ── LED control commands ─────────────────────────────────────────────────

    /// Set LED color via RGB (SET_RGB 0x21). Requires claim.
    ///
    /// Payload: `[R:1][G:1][B:1]` — each 0-255.
    pub async fn set_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<(), AttentioError> {
        self.send_claimed(CMD_SET_RGB, &[r, g, b]).await?;
        Ok(())
    }

    /// Set LED color via HSV (SET_HSV 0x22). Requires claim.
    ///
    /// Payload: `[H:2 little-endian][S:1][V:1]` — H=0-359, S=0-100, V=0-100.
    pub async fn set_hsv(&mut self, h: u16, s: u8, v: u8) -> Result<(), AttentioError> {
        let payload = [
            (h & 0xFF) as u8,        // H low byte
            ((h >> 8) & 0xFF) as u8, // H high byte
            s,
            v,
        ];
        self.send_claimed(CMD_SET_HSV, &payload).await?;
        Ok(())
    }

    /// Set LED brightness (SET_BRIGHTNESS 0x23). Requires claim.
    ///
    /// Payload: `[brightness:1]` — 0-100 (percentage).
    pub async fn set_brightness(&mut self, brightness: u8) -> Result<(), AttentioError> {
        self.send_claimed(CMD_SET_BRIGHTNESS, &[brightness]).await?;
        Ok(())
    }

    /// Turn LEDs off (LED_OFF 0x20). Requires claim.
    pub async fn led_off(&mut self) -> Result<(), AttentioError> {
        self.send_claimed(CMD_LED_OFF, &[]).await?;
        Ok(())
    }

    // ── Power commands ───────────────────────────────────────────────────────

    /// Power on the device (POWER_ON 0x10). Requires claim.
    pub async fn power_on(&mut self) -> Result<(), AttentioError> {
        self.send_claimed(CMD_POWER_ON, &[]).await?;
        Ok(())
    }

    /// Power off the device (POWER_OFF 0x11). Requires claim.
    pub async fn power_off(&mut self) -> Result<(), AttentioError> {
        self.send_claimed(CMD_POWER_OFF, &[]).await?;
        Ok(())
    }

    // ── Settings commands ────────────────────────────────────────────────────

    /// List all settings (SETTINGS_LIST 0x50).
    ///
    /// Same count-prefixed key-value format as GET_METADATA.
    pub async fn settings_list(&mut self) -> Result<Vec<(String, String)>, AttentioError> {
        let payload = self.send_command_ok(CMD_SETTINGS_LIST, &[]).await?;
        parse_kv_list(&payload)
    }

    /// Get a single setting by key (SETTINGS_GET 0x51).
    ///
    /// Request payload: `[key_len:1][key]`
    /// Response payload: `[key_len:1][key][val_len:1][val]` (single pair, no count).
    pub async fn settings_get(&mut self, key: &str) -> Result<(String, String), AttentioError> {
        let mut req = Vec::with_capacity(1 + key.len());
        req.push(key.len() as u8);
        req.extend_from_slice(key.as_bytes());

        let payload = self.send_command_ok(CMD_SETTINGS_GET, &req).await?;
        parse_kv_single(&payload)
    }

    /// Set a setting (SETTINGS_SET 0x52). Requires claim (auto-claims).
    ///
    /// Request payload: `[key_len:1][key][val_len:1][val]`
    /// Response: bare OK (no payload).
    pub async fn settings_set(&mut self, key: &str, value: &str) -> Result<(), AttentioError> {
        let mut req = Vec::with_capacity(2 + key.len() + value.len());
        req.push(key.len() as u8);
        req.extend_from_slice(key.as_bytes());
        req.push(value.len() as u8);
        req.extend_from_slice(value.as_bytes());

        self.send_claimed(CMD_SETTINGS_SET, &req).await?;
        Ok(())
    }

    // ── Log level commands ───────────────────────────────────────────────────

    /// Get the current runtime log level (LOG_GET_LEVEL 0x60).
    ///
    /// Returns the level as a u8: 0=NONE, 1=ERROR, 2=WARN, 3=INFO, 4=DEBUG.
    pub async fn log_get_level(&mut self) -> Result<u8, AttentioError> {
        let payload = self.send_command_ok(CMD_LOG_GET_LEVEL, &[]).await?;
        if payload.is_empty() {
            return Err(AttentioError::Protocol {
                message: "LOG_GET_LEVEL response has no payload".to_string(),
            });
        }
        Ok(payload[0])
    }

    /// Set the runtime log level (LOG_SET_LEVEL 0x61).
    ///
    /// This is ephemeral — the change is lost on reboot. Use
    /// `settings_set("default_loglevel", ...)` for persistent changes.
    ///
    /// Level: 0=NONE, 1=ERROR, 2=WARN, 3=INFO, 4=DEBUG.
    pub async fn log_set_level(&mut self, level: u8) -> Result<(), AttentioError> {
        self.send_command_ok(CMD_LOG_SET_LEVEL, &[level]).await?;
        Ok(())
    }
}

// ── Permanent reader task ────────────────────────────────────────────────────

/// Background task: continuously reads bytes from the AP port, parses them,
/// and dispatches each parsed [`ApResponse`].
///
/// - **Events** (cmd 0x80–0x8F) are broadcast via `monitor_tx` only.
/// - **Command responses** are delivered to the in-flight command waiter (if
///   any) via the oneshot stored in `pending`, AND broadcast via `monitor_tx`
///   so monitor views see them too.
///
/// Exits cleanly when the underlying connection returns 0 bytes (EOF / device
/// unplugged / BLE disconnect) or an I/O error.
async fn reader_loop(
    mut reader: ConnReader,
    monitor_tx: broadcast::Sender<MonitorEvent>,
    pending: Arc<std::sync::Mutex<Option<oneshot::Sender<ApResponse>>>>,
    _guard: Arc<ConnGuard>,
) {
    let mut parser = ApParser::new();
    let mut buf = [0u8; 256];

    loop {
        let n = match reader.read_raw(&mut buf).await {
            Ok(0) => {
                debug!("AP reader: connection closed (EOF)");
                return;
            }
            Ok(n) => n,
            Err(e) => {
                debug!("AP reader: read error: {}", e);
                return;
            }
        };

        for &byte in &buf[..n] {
            if let Some(resp) = parser.feed(byte) {
                // Always broadcast for monitor views.
                let _ = monitor_tx.send(MonitorEvent::Incoming(resp.clone()));

                if resp.is_event() {
                    // Unsolicited event — do NOT deliver to a command waiter.
                    debug!(
                        "AP event: cmd=0x{:02X} payload_len={}",
                        resp.cmd,
                        resp.payload.len()
                    );
                } else {
                    // Command-shaped response (OK 0x00, ERROR 0xFF, or any
                    // other non-event cmd echo). Deliver to the in-flight
                    // command waiter, if any.
                    let waiter = {
                        let mut pending_guard =
                            pending.lock().expect("ApClient pending mutex poisoned");
                        pending_guard.take()
                    };
                    if let Some(tx) = waiter {
                        if tx.send(resp).is_err() {
                            debug!(
                                "AP reader: command waiter dropped before response delivered"
                            );
                        }
                    } else {
                        warn!(
                            "AP reader: received response cmd=0x{:02X} with no in-flight command waiter",
                            resp.cmd
                        );
                    }
                }
            }
        }
    }
}

// ── Binary key-value payload parsing ─────────────────────────────────────────

/// Parse a count-prefixed key-value list from an AP response payload.
///
/// Format: `[count:1] { [key_len:1][key:key_len][val_len:1][val:val_len] } * count`
fn parse_kv_list(data: &[u8]) -> Result<Vec<(String, String)>, AttentioError> {
    if data.is_empty() {
        return Err(AttentioError::Protocol {
            message: "empty key-value list payload".to_string(),
        });
    }

    let count = data[0] as usize;
    let mut pos = 1;
    let mut entries = Vec::with_capacity(count);

    for i in 0..count {
        let (key, value, new_pos) =
            parse_kv_pair(data, pos).map_err(|msg| AttentioError::Protocol {
                message: format!("key-value list entry {}/{}: {}", i + 1, count, msg),
            })?;
        entries.push((key, value));
        pos = new_pos;
    }

    Ok(entries)
}

/// Parse a paginated key-value list from an AP GET_METADATA response payload.
///
/// Format: `[total_count:1][page:1][page_count:1] { [key_len:1][key][val_len:1][val] } * page_count`
///
/// Returns `(total_count, page, entries)`.
fn parse_kv_paginated(data: &[u8]) -> Result<(u8, u8, Vec<(String, String)>), AttentioError> {
    if data.len() < 3 {
        return Err(AttentioError::Protocol {
            message: format!(
                "paginated key-value payload too short: {} bytes (need at least 3)",
                data.len()
            ),
        });
    }

    let total_count = data[0];
    let page = data[1];
    let page_count = data[2] as usize;
    let mut pos = 3;
    let mut entries = Vec::with_capacity(page_count);

    for i in 0..page_count {
        let (key, value, new_pos) =
            parse_kv_pair(data, pos).map_err(|msg| AttentioError::Protocol {
                message: format!(
                    "paginated key-value entry {}/{} (page {}): {}",
                    i + 1,
                    page_count,
                    page,
                    msg
                ),
            })?;
        entries.push((key, value));
        pos = new_pos;
    }

    Ok((total_count, page, entries))
}

/// Parse a single key-value pair (no count prefix) from an AP response payload.
///
/// Format: `[key_len:1][key:key_len][val_len:1][val:val_len]`
fn parse_kv_single(data: &[u8]) -> Result<(String, String), AttentioError> {
    if data.is_empty() {
        return Err(AttentioError::Protocol {
            message: "empty key-value payload".to_string(),
        });
    }

    let (key, value, _) =
        parse_kv_pair(data, 0).map_err(|msg| AttentioError::Protocol { message: msg })?;
    Ok((key, value))
}

/// Parse one `[key_len][key][val_len][val]` pair starting at `pos`.
///
/// Returns `(key, value, new_pos)` on success, or an error message string.
fn parse_kv_pair(data: &[u8], pos: usize) -> Result<(String, String, usize), String> {
    if pos >= data.len() {
        return Err("unexpected end of data (no key_len)".to_string());
    }
    let key_len = data[pos] as usize;
    let key_start = pos + 1;
    let key_end = key_start + key_len;
    if key_end > data.len() {
        return Err(format!(
            "key_len={} exceeds remaining data ({} bytes)",
            key_len,
            data.len() - key_start
        ));
    }
    let key = String::from_utf8(data[key_start..key_end].to_vec())
        .map_err(|_| "key is not valid UTF-8".to_string())?;

    if key_end >= data.len() {
        return Err("unexpected end of data (no val_len)".to_string());
    }
    let val_len = data[key_end] as usize;
    let val_start = key_end + 1;
    let val_end = val_start + val_len;
    if val_end > data.len() {
        return Err(format!(
            "val_len={} exceeds remaining data ({} bytes)",
            val_len,
            data.len() - val_start
        ));
    }
    let value = String::from_utf8(data[val_start..val_end].to_vec())
        .map_err(|_| "value is not valid UTF-8".to_string())?;

    Ok((key, value, val_end))
}
