use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::device::connection::DeviceConnection;
use crate::device::discovery::resolve_device;
use crate::error::AttentioError;
use crate::monitor::app::{Action, App};
use crate::monitor::format;
use crate::monitor::{event as monitor_event, ui};
use crate::protocol::packet::{build_packet, ApParser, CMD_LOG_GET_LEVEL, CMD_LOG_SET_LEVEL};

/// Interval between reconnection attempts for disconnected ports.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(3);

/// Messages sent from background reader tasks to the main event loop.
enum ReaderMsg {
    // ── CDC0 (serial prints) ─────────────────────────────────────────────
    /// A line received from the serial prints port (CDC0).
    SerialLine(String),
    /// The serial port reader encountered an unrecoverable error.
    SerialError(String),
    /// The serial port is busy — another process has it open.
    SerialPortBusy(String),
    /// The serial port was successfully reconnected.
    SerialReconnected,

    // ── CDC1 (AP protocol) ───────────────────────────────────────────────
    /// A formatted AP traffic line to display (incoming response/event).
    ApLine(String),
    /// The AP port reader encountered an unrecoverable error.
    ApError(String),
    /// The AP port is busy.
    ApPortBusy(String),
    /// The AP port was successfully (re)connected.
    ApConnected,
}

enum OpenPortResult {
    Connected(DeviceConnection),
    Busy,
    Failed,
}

/// Restore the terminal to its normal state.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = io::stdout().execute(LeaveAlternateScreen);
}

fn try_open_port(path: &str, label: &str, timeout: Duration) -> OpenPortResult {
    match DeviceConnection::open(path) {
        Ok(conn) => {
            info!("{} opened: {}", label, path);
            OpenPortResult::Connected(conn.with_timeout(timeout))
        }
        Err(e) if e.is_port_busy() => {
            warn!("{}", e);
            OpenPortResult::Busy
        }
        Err(e) => {
            warn!("Failed to open {} {}: {}", label.to_lowercase(), path, e);
            OpenPortResult::Failed
        }
    }
}

/// Execute the `monitor` command — two-pane monitor dashboard.
///
/// Top pane: AP protocol traffic (CDC1) — commands, responses, events.
/// Bottom pane: serial prints (CDC0).
pub async fn execute(device: Option<&str>) -> Result<()> {
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;

    let serial_port_path = dev.serial_port().map(|s| s.to_string());
    let ap_port_path = dev.ap_port().map(|s| s.to_string());

    info!(
        "Starting monitor for {} — serial: {}, ap: {}",
        dev.serial,
        serial_port_path.as_deref().unwrap_or("none"),
        ap_port_path.as_deref().unwrap_or("none"),
    );

    let (tx, mut rx) = mpsc::channel::<ReaderMsg>(256);

    // Channel for sending AP commands from the main loop to the AP writer task.
    let (ap_cmd_tx, ap_cmd_rx) = mpsc::channel::<Vec<u8>>(16);

    let mut task_handles: Vec<JoinHandle<()>> = Vec::new();

    // --- Try to open CDC0 (serial prints) ---
    let mut serial_connected = false;
    let mut serial_reconnecting = false;
    let mut serial_port_busy = false;
    if let Some(ref path) = serial_port_path {
        match try_open_port(path, "Serial port", Duration::from_millis(500)) {
            OpenPortResult::Connected(conn) => {
                serial_connected = true;
                let tx_serial = tx.clone();
                let handle = tokio::spawn(async move {
                    serial_reader_task(conn, tx_serial).await;
                });
                task_handles.push(handle);
            }
            OpenPortResult::Busy => {
                serial_port_busy = true;
                let tx_reconnect = tx.clone();
                let path_clone = path.clone();
                let handle = tokio::spawn(async move {
                    serial_reconnect_task(path_clone, tx_reconnect).await;
                });
                task_handles.push(handle);
            }
            OpenPortResult::Failed => {
                serial_reconnecting = true;
                let tx_reconnect = tx.clone();
                let path_clone = path.clone();
                let handle = tokio::spawn(async move {
                    serial_reconnect_task(path_clone, tx_reconnect).await;
                });
                task_handles.push(handle);
            }
        }
    }

    // --- Try to open CDC1 (AP protocol) ---
    let mut ap_connected = false;
    let mut ap_reconnecting = false;
    let mut ap_port_busy = false;
    if let Some(ref path) = ap_port_path {
        match try_open_port(path, "AP port", Duration::from_millis(100)) {
            OpenPortResult::Connected(conn) => {
                ap_connected = true;
                let tx_ap = tx.clone();
                let handle = tokio::spawn(async move {
                    ap_reader_writer_task(conn, tx_ap, ap_cmd_rx).await;
                });
                task_handles.push(handle);
            }
            OpenPortResult::Busy => {
                ap_port_busy = true;
                let tx_reconnect = tx.clone();
                let path_clone = path.clone();
                let handle = tokio::spawn(async move {
                    ap_reconnect_task(path_clone, tx_reconnect, ap_cmd_rx).await;
                });
                task_handles.push(handle);
            }
            OpenPortResult::Failed => {
                ap_reconnecting = true;
                let tx_reconnect = tx.clone();
                let path_clone = path.clone();
                let handle = tokio::spawn(async move {
                    ap_reconnect_task(path_clone, tx_reconnect, ap_cmd_rx).await;
                });
                task_handles.push(handle);
            }
        }
    }

    // Create app state
    let mut app = App::new(
        dev.serial.clone(),
        serial_port_path,
        serial_connected,
        ap_port_path,
    );
    app.serial_reconnecting = serial_reconnecting;
    app.serial_port_busy = serial_port_busy;
    app.ap_connected = ap_connected;
    app.ap_reconnecting = ap_reconnecting;
    app.ap_port_busy = ap_port_busy;

    // Query initial log level via the shared AP connection
    if ap_connected {
        let pkt = build_packet(CMD_LOG_GET_LEVEL, &[]);
        let _ = ap_cmd_tx.send(pkt).await;
    }

    // Terminal event polling
    let (term_tx, mut term_rx) = mpsc::channel::<Event>(64);
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = shutdown.clone();

    let term_handle = tokio::task::spawn_blocking(move || {
        while !shutdown_flag.load(Ordering::Relaxed) {
            match event::poll(Duration::from_millis(50)) {
                Ok(true) => match event::read() {
                    Ok(evt) => {
                        if term_tx.blocking_send(evt).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => continue,
                Err(_) => break,
            }
        }
    });

    // Enter monitor mode
    let result = async {
        enable_raw_mode().context("failed to enable raw mode")?;
        io::stdout()
            .execute(EnterAlternateScreen)
            .context("failed to enter alternate screen")?;

        run_event_loop(
            &mut app,
            &mut rx,
            &mut term_rx,
            tx.clone(),
            &ap_cmd_tx,
            &mut task_handles,
        )
        .await
    }
    .await;

    // Cleanup
    shutdown.store(true, Ordering::Relaxed);

    for handle in &task_handles {
        handle.abort();
    }
    let _ = tokio::time::timeout(Duration::from_millis(300), async {
        for handle in task_handles {
            let _ = handle.await;
        }
    })
    .await;

    let _ = tokio::time::timeout(Duration::from_millis(100), term_handle).await;

    restore_terminal();
    eprintln!("Monitor session ended.");

    result
}

/// The main event loop.
async fn run_event_loop(
    app: &mut App,
    rx: &mut mpsc::Receiver<ReaderMsg>,
    term_rx: &mut mpsc::Receiver<Event>,
    reader_tx: mpsc::Sender<ReaderMsg>,
    ap_cmd_tx: &mpsc::Sender<Vec<u8>>,
    task_handles: &mut Vec<JoinHandle<()>>,
) -> Result<()> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    if app.serial_connected {
        app.push_serial_line("Listening for serial prints...".to_string());
    }
    if app.ap_connected {
        app.push_ap_line("Listening for AP traffic...".to_string());
    }

    app.push_serial_line("Press Esc or Ctrl+C to quit. Tab to switch panes.".to_string());
    app.push_serial_line(String::new());

    terminal.draw(|frame| ui::render(frame, app))?;

    while app.running {
        let needs_render = tokio::select! {
            maybe_event = term_rx.recv() => {
                match maybe_event {
                    Some(Event::Key(key)) => {
                        if key.kind == KeyEventKind::Press {
                            let action = monitor_event::handle_key_event(app, key);
                            match action {
                                Action::Quit => break,
                                Action::SetLogLevel(level) => {
                                    handle_set_log_level(app, ap_cmd_tx, level).await;
                                }
                                Action::None => {}
                            }
                            true
                        } else {
                            false
                        }
                    }
                    Some(Event::Resize(_, _)) => true,
                    Some(_) => false,
                    None => break,
                }
            }

            Some(msg) = rx.recv() => {
                match msg {
                    // ── CDC0 messages ────────────────────────────────
                    ReaderMsg::SerialLine(line) => {
                        app.push_serial_line(line);
                    }
                    ReaderMsg::SerialError(err) => {
                        app.push_serial_line(format!("[ERROR: {}]", err));
                        app.serial_connected = false;
                        let path_clone = app.serial_port_path.clone();
                        if let Some(path) = path_clone {
                            app.serial_reconnecting = true;
                            app.push_serial_line("Attempting to reconnect...".to_string());
                            let tx_reconnect = reader_tx.clone();
                            let handle = tokio::spawn(async move {
                                serial_reconnect_task(path, tx_reconnect).await;
                            });
                            task_handles.push(handle);
                        }
                    }
                    ReaderMsg::SerialReconnected => {
                        app.serial_connected = true;
                        app.serial_reconnecting = false;
                        app.serial_port_busy = false;
                        app.push_serial_line("Reconnected. Listening for serial prints...".to_string());
                    }
                    ReaderMsg::SerialPortBusy(msg) => {
                        app.push_serial_line(format!("[{}]", msg));
                        app.serial_reconnecting = false;
                        app.serial_port_busy = true;
                    }

                    // ── CDC1 messages ────────────────────────────────
                    ReaderMsg::ApLine(line) => {
                        // Check if this is a LOG_GET_LEVEL response to update the status bar
                        parse_log_level_from_line(&line, app);
                        app.push_ap_line(line);
                    }
                    ReaderMsg::ApError(err) => {
                        app.push_ap_line(format!("[ERROR: {}]", err));
                        app.ap_connected = false;
                        app.ap_reconnecting = true;
                    }
                    ReaderMsg::ApPortBusy(msg) => {
                        app.push_ap_line(format!("[{}]", msg));
                        app.ap_reconnecting = false;
                        app.ap_port_busy = true;
                    }
                    ReaderMsg::ApConnected => {
                        app.ap_connected = true;
                        app.ap_reconnecting = false;
                        app.ap_port_busy = false;
                        app.push_ap_line("Connected. Listening for AP traffic...".to_string());
                        // Query log level now that we're connected
                        let pkt = build_packet(CMD_LOG_GET_LEVEL, &[]);
                        let _ = ap_cmd_tx.send(pkt).await;
                    }
                }
                true
            }
        };

        if needs_render {
            terminal.draw(|frame| ui::render(frame, app))?;
        }
    }

    Ok(())
}

/// Handle log level set command via the unified AP connection.
async fn handle_set_log_level(app: &mut App, ap_cmd_tx: &mpsc::Sender<Vec<u8>>, level: u8) {
    if !app.ap_connected {
        app.push_ap_line("[No AP connection for log level control]".to_string());
        return;
    }

    let pkt = build_packet(CMD_LOG_SET_LEVEL, &[level]);
    if ap_cmd_tx.send(pkt).await.is_err() {
        app.push_ap_line("[Failed to send log level command]".to_string());
        return;
    }

    // Optimistically update — the response will confirm in the AP pane.
    // The outgoing command is displayed by ap_reader_writer_task when it
    // dequeues the packet, so we don't push an AP line here.
    app.log_level = Some(level);
    let name = super::loglevel::level_name(level);
    app.push_serial_line(format!("[Log level set to {} ({})]", level, name));
}

/// Try to extract log level from a LOG_GET_LEVEL OK response line.
fn parse_log_level_from_line(line: &str, app: &mut App) {
    if line.starts_with("← OK [") && line.len() >= 8 {
        let hex_part = line.trim_start_matches("← OK [").trim_end_matches(']');
        if hex_part.len() == 2 {
            if let Ok(level) = u8::from_str_radix(hex_part, 16) {
                if level <= 4 {
                    app.log_level = Some(level);
                }
            }
        }
    }
}

// ── Background tasks ─────────────────────────────────────────────────────────

/// Background task: continuously reads lines from the serial port (CDC0).
async fn serial_reader_task(mut conn: DeviceConnection, tx: mpsc::Sender<ReaderMsg>) {
    loop {
        match conn.read_line().await {
            Ok(line) => {
                if tx.send(ReaderMsg::SerialLine(line)).await.is_err() {
                    break;
                }
            }
            Err(AttentioError::Timeout { .. }) => continue,
            Err(e) => {
                let _ = tx.send(ReaderMsg::SerialError(e.to_string())).await;
                break;
            }
        }
    }
    debug!("Serial reader task exiting");
}

/// Background task: reconnects the serial port (CDC0).
async fn serial_reconnect_task(port_path: String, tx: mpsc::Sender<ReaderMsg>) {
    loop {
        tokio::time::sleep(RECONNECT_INTERVAL).await;
        match try_open_port(&port_path, "Serial port", Duration::from_millis(500)) {
            OpenPortResult::Connected(conn) => {
                if tx.send(ReaderMsg::SerialReconnected).await.is_err() {
                    return;
                }
                serial_reader_task(conn, tx).await;
                return;
            }
            OpenPortResult::Busy => {
                let _ = tx
                    .send(ReaderMsg::SerialPortBusy(format!(
                        "port {} is busy — another process has it open",
                        port_path
                    )))
                    .await;
            }
            OpenPortResult::Failed => {}
        }
    }
}

/// Background task: reads AP responses/events from CDC1 and sends outgoing
/// commands from the command channel.
///
/// This task owns the DeviceConnection for CDC1 and multiplexes reads and
/// writes. Incoming AP packets are parsed and formatted, then sent as ApLine
/// messages. Outgoing commands from the main loop (e.g., log level changes)
/// are written to the port, and their formatted representation is also sent
/// as ApLine messages.
async fn ap_reader_writer_task(
    mut conn: DeviceConnection,
    tx: mpsc::Sender<ReaderMsg>,
    mut cmd_rx: mpsc::Receiver<Vec<u8>>,
) {
    let mut parser = ApParser::new();
    let mut buf = [0u8; 256];

    loop {
        tokio::select! {
            // Read incoming bytes from the device
            result = conn.read_raw(&mut buf) => {
                match result {
                    Ok(0) => {
                        let _ = tx.send(ReaderMsg::ApError("AP connection closed".to_string())).await;
                        break;
                    }
                    Ok(n) => {
                        for &byte in &buf[..n] {
                            if let Some(resp) = parser.feed(byte) {
                                let line = format::format_incoming(&resp);
                                if tx.send(ReaderMsg::ApLine(line)).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Err(AttentioError::Timeout { .. }) => continue,
                    Err(e) => {
                        let _ = tx.send(ReaderMsg::ApError(e.to_string())).await;
                        break;
                    }
                }
            }

            // Send outgoing commands
            Some(pkt) = cmd_rx.recv() => {
                // Format the outgoing command for display (extract cmd and payload from packet)
                if pkt.len() >= 4 {
                    let cmd = pkt[2];
                    let payload = if pkt.len() > 4 { &pkt[3..pkt.len()-1] } else { &[] };
                    let line = format::format_outgoing(cmd, payload);
                    let _ = tx.send(ReaderMsg::ApLine(line)).await;
                }
                if let Err(e) = conn.write_raw(&pkt).await {
                    let _ = tx.send(ReaderMsg::ApLine(format!("[Write error: {}]", e))).await;
                }
            }
        }
    }
    debug!("AP reader/writer task exiting");
}

/// Background task: reconnects CDC1 AP port.
async fn ap_reconnect_task(
    port_path: String,
    tx: mpsc::Sender<ReaderMsg>,
    cmd_rx: mpsc::Receiver<Vec<u8>>,
) {
    loop {
        tokio::time::sleep(RECONNECT_INTERVAL).await;
        match try_open_port(&port_path, "AP port", Duration::from_millis(100)) {
            OpenPortResult::Connected(conn) => {
                if tx.send(ReaderMsg::ApConnected).await.is_err() {
                    return;
                }
                ap_reader_writer_task(conn, tx, cmd_rx).await;
                return;
            }
            OpenPortResult::Busy => {
                let _ = tx
                    .send(ReaderMsg::ApPortBusy(format!(
                        "port {} is busy — another process has it open",
                        port_path
                    )))
                    .await;
            }
            OpenPortResult::Failed => {}
        }
    }
}
