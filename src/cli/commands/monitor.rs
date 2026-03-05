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

/// Messages sent from background reader tasks to the main event loop.
enum ReaderMsg {
    /// A line received from the debug prints port (CDC0).
    DebugLine(String),
    /// A line received from the shell port (CDC1) — part of a command response.
    ShellLine(String),
    /// The debug port reader encountered an unrecoverable error.
    DebugError(String),
    /// The shell port reader encountered an unrecoverable error.
    ShellError(String),
}

/// Restore the terminal to its normal state.
/// Called on both clean exit and error paths.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = io::stdout().execute(LeaveAlternateScreen);
}

/// Execute the `monitor` command — TUI dashboard with dual CDC view.
///
/// Always shows both panes (debug prints + shell). Each CDC port is opened
/// independently — if one fails, that pane shows "(not connected)" while the
/// other continues to work.
pub async fn execute(device: Option<&str>) -> Result<()> {
    // Resolve which device to talk to
    let dev = resolve_device(device).context("failed to resolve device")?;

    let debug_port_path = dev.debug_port().map(|s| s.to_string());
    let shell_port_path = dev.shell_port().map(|s| s.to_string());

    info!(
        "Starting monitor for {} — debug: {}, shell: {}",
        dev.serial,
        debug_port_path.as_deref().unwrap_or("none"),
        shell_port_path.as_deref().unwrap_or("none"),
    );

    // Channel for background reader tasks to send messages to the main loop
    let (tx, mut rx) = mpsc::channel::<ReaderMsg>(256);

    // Track spawned tasks so we can abort them on exit
    let mut task_handles: Vec<JoinHandle<()>> = Vec::new();

    // --- Try to open CDC0 (debug prints) ---
    let mut debug_connected = false;
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
            Err(e) => {
                warn!("Failed to open debug port {}: {}", path, e);
            }
        }
    }

    // --- Try to open CDC1 (shell) ---
    let mut shell_connected = false;
    let cmd_tx = if let Some(ref path) = shell_port_path {
        match DeviceConnection::open(path) {
            Ok(conn) => {
                shell_connected = true;
                info!("Shell port opened: {}", path);

                let (cmd_tx, cmd_rx) = mpsc::channel::<String>(32);
                let tx_shell = tx.clone();

                let handle = tokio::spawn(async move {
                    shell_io_task(conn, cmd_rx, tx_shell).await;
                });
                task_handles.push(handle);

                Some(cmd_tx)
            }
            Err(e) => {
                warn!("Failed to open shell port {}: {}", path, e);
                None
            }
        }
    } else {
        None
    };

    // Create app state with connection status
    let mut app = App::new(
        dev.serial.clone(),
        debug_port_path,
        shell_port_path,
        debug_connected,
        shell_connected,
    );

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

    // Enter TUI mode
    enable_raw_mode().context("failed to enable raw mode")?;
    io::stdout()
        .execute(EnterAlternateScreen)
        .context("failed to enter alternate screen")?;

    // Run the main loop, capturing the result
    let result = run_event_loop(&mut app, &mut rx, &mut term_rx, cmd_tx).await;

    // Signal the terminal polling thread to stop, then abort async tasks
    shutdown.store(true, Ordering::Relaxed);
    for handle in &task_handles {
        handle.abort();
    }
    // Wait briefly for the polling thread to notice the shutdown flag
    let _ = tokio::time::timeout(Duration::from_millis(200), term_handle).await;

    // Restore terminal — always, regardless of success or failure
    restore_terminal();

    eprintln!("Monitor session ended.");

    result
}

/// The main TUI event loop. Separated so we can guarantee terminal restore via the caller.
async fn run_event_loop(
    app: &mut App,
    rx: &mut mpsc::Receiver<ReaderMsg>,
    term_rx: &mut mpsc::Receiver<Event>,
    cmd_tx: Option<mpsc::Sender<String>>,
) -> Result<()> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    // Add initial status messages
    if app.debug_connected {
        app.push_debug_line("Listening for debug prints...".to_string());
    }

    if app.shell_connected {
        app.push_shell_line("Shell ready. Type commands below.".to_string());
    }

    app.push_shell_line("Press Esc or Ctrl+C to quit.".to_string());
    app.push_shell_line(String::new());

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
                                Action::SendCommand(cmd) => {
                                    if let Some(ref cmd_tx) = cmd_tx {
                                        if cmd_tx.send(cmd).await.is_err() {
                                            app.push_shell_line(
                                                "[ERROR: shell connection lost]".to_string()
                                            );
                                            app.shell_connected = false;
                                        }
                                    }
                                }
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
                    ReaderMsg::ShellLine(line) => {
                        app.push_shell_line(line);
                    }
                    ReaderMsg::DebugError(err) => {
                        app.push_debug_line(format!("[ERROR: {}]", err));
                        app.debug_connected = false;
                    }
                    ReaderMsg::ShellError(err) => {
                        app.push_shell_line(format!("[ERROR: {}]", err));
                    }
                }
                true
            }
        };

        if needs_render {
            terminal.draw(|frame| ui::render(frame, app))?;
        }
    }

    // Drop cmd_tx so shell_io_task's recv() returns None and it exits
    drop(cmd_tx);

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

/// Background task that handles shell I/O: receives commands from the main loop,
/// sends them to the device, and forwards response lines back.
async fn shell_io_task(
    mut conn: DeviceConnection,
    mut cmd_rx: mpsc::Receiver<String>,
    tx: mpsc::Sender<ReaderMsg>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        match conn.send_command(&cmd).await {
            Ok(response) => {
                if !response.is_empty() {
                    for line in response.lines() {
                        if tx
                            .send(ReaderMsg::ShellLine(line.to_string()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                // Show OK marker
                if tx
                    .send(ReaderMsg::ShellLine("OK".to_string()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(e) => {
                if tx.send(ReaderMsg::ShellError(e.to_string())).await.is_err() {
                    return;
                }
            }
        }
    }
    debug!("Shell I/O task exiting");
}
