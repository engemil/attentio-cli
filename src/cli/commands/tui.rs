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
use crate::tui::app::{Action, App};
use crate::tui::{event as tui_event, ui};

/// Interval between reconnection attempts for disconnected ports.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(3);

/// Messages sent from background reader tasks to the main event loop.
enum ReaderMsg {
    /// A line received from the debug prints port (CDC0).
    DebugLine(String),
    /// The debug port reader encountered an unrecoverable error.
    DebugError(String),
    /// The debug port is busy — another process has it open.
    DebugPortBusy(String),
    /// The debug port was successfully reconnected.
    DebugReconnected,
}

/// Restore the terminal to its normal state.
/// Called on both clean exit and error paths.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = io::stdout().execute(LeaveAlternateScreen);
}

/// Execute the `tui` command — TUI dashboard with CDC debug view.
///
/// Shows a single full-height pane for debug prints (CDC0). If the port
/// fails to open, a background task retries every few seconds until it
/// becomes available.
pub async fn execute(device: Option<&str>) -> Result<()> {
    // Resolve which device to talk to
    let dev = resolve_device(device)
        .await
        .context("failed to resolve device")?;

    let debug_port_path = dev.debug_port().map(|s| s.to_string());

    info!(
        "Starting TUI for {} — debug: {}",
        dev.serial,
        debug_port_path.as_deref().unwrap_or("none"),
    );

    // Channel for background reader tasks to send messages to the main loop
    let (tx, mut rx) = mpsc::channel::<ReaderMsg>(256);

    // Track spawned tasks so we can abort them on exit
    let mut task_handles: Vec<JoinHandle<()>> = Vec::new();

    // --- Try to open CDC0 (debug prints) ---
    let mut debug_connected = false;
    let mut debug_reconnecting = false;
    let mut debug_port_busy = false;
    if let Some(ref path) = debug_port_path {
        match DeviceConnection::open(path) {
            Ok(conn) => {
                let conn = conn.with_timeout(Duration::from_millis(500));
                debug_connected = true;
                info!("Debug port opened: {}", path);

                let tx_debug = tx.clone();
                let handle = tokio::spawn(async move {
                    debug_reader_task(conn, tx_debug).await;
                });
                task_handles.push(handle);
            }
            Err(e) if e.is_port_busy() => {
                warn!("{}", e);
                debug_port_busy = true;
                // Start reconnect task — port may become available later
                let tx_reconnect = tx.clone();
                let path_clone = path.clone();
                let handle = tokio::spawn(async move {
                    debug_reconnect_task(path_clone, tx_reconnect).await;
                });
                task_handles.push(handle);
            }
            Err(e) => {
                warn!("Failed to open debug port {}: {}", path, e);
                // Start reconnect task in background
                debug_reconnecting = true;
                let tx_reconnect = tx.clone();
                let path_clone = path.clone();
                let handle = tokio::spawn(async move {
                    debug_reconnect_task(path_clone, tx_reconnect).await;
                });
                task_handles.push(handle);
            }
        }
    }

    // Create app state with connection status
    let mut app = App::new(dev.serial.clone(), debug_port_path, debug_connected);
    app.debug_reconnecting = debug_reconnecting;
    app.debug_port_busy = debug_port_busy;

    // Channel for terminal events (polled from a blocking thread)
    let (term_tx, mut term_rx) = mpsc::channel::<Event>(64);

    // Shared shutdown flag — signals the polling thread to exit
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = shutdown.clone();

    // Spawn a blocking thread to poll crossterm events.
    // This works reliably across containers, SSH, and all terminal types.
    let term_handle = tokio::task::spawn_blocking(move || {
        while !shutdown_flag.load(Ordering::Relaxed) {
            // Poll with 50ms timeout — short enough to check shutdown promptly
            match event::poll(Duration::from_millis(50)) {
                Ok(true) => match event::read() {
                    Ok(evt) => {
                        if term_tx.blocking_send(evt).is_err() {
                            // Receiver dropped — main loop exited
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => continue, // No event within timeout, poll again
                Err(_) => break,       // Terminal error
            }
        }
    });

    // Enter TUI mode and run the event loop.
    // Wrapped so that cleanup (task abort, terminal restore) always runs,
    // even if enable_raw_mode() or EnterAlternateScreen fails.
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
            &mut task_handles,
        )
        .await
    }
    .await;

    // Always clean up: signal the terminal polling thread to stop, abort async tasks.
    // This runs whether the TUI launched successfully or failed at setup.
    shutdown.store(true, Ordering::Relaxed);

    // Abort each async task and then await it so the tokio runtime actually
    // drives the cancellation future to completion and drops the DeviceConnection
    // (which clears TIOCEXCL). Without awaiting, the fd may still be open when
    // the next command runs its /proc busy-check.
    for handle in task_handles {
        handle.abort();
        // Ignore JoinError — an aborted task always returns Err(JoinError::Cancelled)
        let _ = tokio::time::timeout(Duration::from_millis(500), handle).await;
    }

    // Wait briefly for the polling thread to notice the shutdown flag
    let _ = tokio::time::timeout(Duration::from_millis(200), term_handle).await;

    // Restore terminal — always, regardless of success or failure
    restore_terminal();

    eprintln!("TUI session ended.");

    result
}

/// The main TUI event loop. Separated so we can guarantee terminal restore via the caller.
async fn run_event_loop(
    app: &mut App,
    rx: &mut mpsc::Receiver<ReaderMsg>,
    term_rx: &mut mpsc::Receiver<Event>,
    reader_tx: mpsc::Sender<ReaderMsg>,
    task_handles: &mut Vec<JoinHandle<()>>,
) -> Result<()> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    // Add initial status messages
    if app.debug_connected {
        app.push_debug_line("Listening for debug prints...".to_string());
    }

    app.push_debug_line("Press Esc or Ctrl+C to quit.".to_string());
    app.push_debug_line(String::new());

    // Initial render
    terminal.draw(|frame| ui::render(frame, app))?;

    while app.running {
        let needs_render = tokio::select! {
            // Terminal events from the blocking poller thread
            maybe_event = term_rx.recv() => {
                match maybe_event {
                    Some(Event::Key(key)) => {
                        if key.kind == KeyEventKind::Press {
                            let action = tui_event::handle_key_event(app, key);
                            match action {
                                Action::Quit => break,
                                Action::None => {}
                            }
                            true
                        } else {
                            false
                        }
                    }
                    Some(Event::Resize(_, _)) => true,
                    Some(_) => false, // Mouse events, etc.
                    None => break,    // Poller thread exited
                }
            }

            // Messages from background CDC reader tasks
            Some(msg) = rx.recv() => {
                match msg {
                    ReaderMsg::DebugLine(line) => {
                        app.push_debug_line(line);
                    }
                    ReaderMsg::DebugError(err) => {
                        app.push_debug_line(format!("[ERROR: {}]", err));
                        app.debug_connected = false;

                        // Start reconnection if we have a port path
                        let path_clone = app.debug_port_path.clone();
                        if let Some(path) = path_clone {
                            app.debug_reconnecting = true;
                            app.push_debug_line("Attempting to reconnect...".to_string());
                            let tx_reconnect = reader_tx.clone();
                            let handle = tokio::spawn(async move {
                                debug_reconnect_task(path, tx_reconnect).await;
                            });
                            task_handles.push(handle);
                        }
                    }
                    ReaderMsg::DebugReconnected => {
                        app.debug_connected = true;
                        app.debug_reconnecting = false;
                        app.debug_port_busy = false;
                        app.push_debug_line("Reconnected. Listening for debug prints...".to_string());
                        info!("Debug port reconnected");
                    }
                    ReaderMsg::DebugPortBusy(msg) => {
                        app.push_debug_line(format!("[{}]", msg));
                        app.debug_reconnecting = false;
                        app.debug_port_busy = true;
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

/// Background task that continuously reads lines from the debug port (CDC0)
/// and sends them to the main loop via the channel.
async fn debug_reader_task(mut conn: DeviceConnection, tx: mpsc::Sender<ReaderMsg>) {
    loop {
        match conn.read_line().await {
            Ok(line) => {
                if tx.send(ReaderMsg::DebugLine(line)).await.is_err() {
                    // Main loop has exited
                    break;
                }
            }
            Err(AttentioError::Timeout { .. }) => {
                // No data within timeout — this is normal for sporadic debug prints.
                // Just continue polling.
                continue;
            }
            Err(e) => {
                let _ = tx.send(ReaderMsg::DebugError(e.to_string())).await;
                break;
            }
        }
    }
    debug!("Debug reader task exiting");
}

/// Background task that periodically attempts to reconnect the debug port (CDC0).
///
/// On success, spawns a new `debug_reader_task` and notifies the main loop.
/// If the port is busy (held by another process), notifies the main loop so the
/// TUI can display the busy status, then keeps retrying in case the port is
/// released.
async fn debug_reconnect_task(port_path: String, tx: mpsc::Sender<ReaderMsg>) {
    loop {
        tokio::time::sleep(RECONNECT_INTERVAL).await;

        match DeviceConnection::open(&port_path) {
            Ok(conn) => {
                let conn = conn.with_timeout(Duration::from_millis(500));
                info!("Debug port reconnected: {}", port_path);

                // Notify the main loop before spawning the reader.
                // If the main loop is gone, just exit.
                if tx.send(ReaderMsg::DebugReconnected).await.is_err() {
                    return;
                }

                // Spawn the reader task in-line (it takes over this task's role)
                debug_reader_task(conn, tx).await;
                return;
            }
            Err(e) if e.is_port_busy() => {
                // Port is held by another process — notify TUI and keep retrying.
                let _ = tx.send(ReaderMsg::DebugPortBusy(e.to_string())).await;
                continue;
            }
            Err(_) => {
                // Port still unavailable — silently retry
                continue;
            }
        }
    }
}
