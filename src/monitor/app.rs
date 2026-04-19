/// Maximum number of lines retained in each log buffer.
const MAX_LINES: usize = 1000;

/// Which pane is currently focused for scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// Top pane — AP protocol traffic (CDC1).
    Protocol,
    /// Bottom pane — serial prints (CDC0).
    Serial,
}

/// Actions produced by event handling that the main loop must act on.
#[derive(Debug)]
pub enum Action {
    /// No external action needed.
    None,
    /// User requested quit.
    Quit,
    /// User requested a runtime log level change.
    SetLogLevel(u8),
}

/// Application state for the TUI.
pub struct App {
    // ── Serial pane (CDC0) ───────────────────────────────────────────────
    /// CDC0 serial print lines (oldest first).
    pub serial_lines: Vec<String>,
    /// Scroll offset for the serial pane (0 = pinned to bottom / latest).
    pub serial_scroll: usize,
    /// Whether the serial port (CDC0) is connected.
    pub serial_connected: bool,
    /// Whether the serial port is currently attempting to reconnect.
    pub serial_reconnecting: bool,
    /// Whether the serial port is busy (held by another process).
    pub serial_port_busy: bool,

    // ── AP pane (CDC1) ───────────────────────────────────────────────────
    /// AP protocol traffic lines (oldest first).
    pub ap_lines: Vec<String>,
    /// Scroll offset for the AP pane (0 = pinned to bottom / latest).
    pub ap_scroll: usize,
    /// Whether the AP port (CDC1) is connected.
    pub ap_connected: bool,
    /// Whether the AP port is currently attempting to reconnect.
    pub ap_reconnecting: bool,
    /// Whether the AP port is busy (held by another process).
    pub ap_port_busy: bool,

    // ── Shared ───────────────────────────────────────────────────────────
    /// Which pane currently has focus for scrolling.
    pub active_pane: Pane,
    /// Whether the TUI is still running.
    pub running: bool,
    /// Device serial number (for display).
    pub device_serial: String,
    /// CDC0 port path (for display).
    pub serial_port_path: Option<String>,
    /// AP protocol port path (CDC1).
    pub ap_port_path: Option<String>,
    /// Current runtime log level (None if not yet queried).
    pub log_level: Option<u8>,
}

impl App {
    /// Create a new App with the given device info.
    pub fn new(
        device_serial: String,
        serial_port_path: Option<String>,
        serial_connected: bool,
        ap_port_path: Option<String>,
    ) -> Self {
        Self {
            serial_lines: Vec::new(),
            serial_scroll: 0,
            serial_connected,
            serial_reconnecting: false,
            serial_port_busy: false,
            ap_lines: Vec::new(),
            ap_scroll: 0,
            ap_connected: false,
            ap_reconnecting: false,
            ap_port_busy: false,
            active_pane: Pane::Serial,
            running: true,
            device_serial,
            serial_port_path,
            ap_port_path,
            log_level: None,
        }
    }

    /// Append a line to the serial log buffer.
    pub fn push_serial_line(&mut self, line: String) {
        push_line(&mut self.serial_lines, &mut self.serial_scroll, line);
    }

    /// Append a line to the AP traffic buffer.
    pub fn push_ap_line(&mut self, line: String) {
        push_line(&mut self.ap_lines, &mut self.ap_scroll, line);
    }

    // ── Scroll helpers ───────────────────────────────────────────────────

    /// Scroll the active pane up by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        let (scroll, total) = self.active_scroll_mut();
        *scroll = scroll.saturating_add(n).min(total);
    }

    /// Scroll the active pane down by `n` lines (towards latest).
    pub fn scroll_down(&mut self, n: usize) {
        let (scroll, _) = self.active_scroll_mut();
        *scroll = scroll.saturating_sub(n);
    }

    /// Toggle the focused pane.
    pub fn toggle_pane(&mut self) {
        self.active_pane = match self.active_pane {
            Pane::Protocol => Pane::Serial,
            Pane::Serial => Pane::Protocol,
        };
    }

    /// Returns a mutable reference to the scroll offset and total line count
    /// for the currently active pane.
    fn active_scroll_mut(&mut self) -> (&mut usize, usize) {
        match self.active_pane {
            Pane::Protocol => (&mut self.ap_scroll, self.ap_lines.len()),
            Pane::Serial => (&mut self.serial_scroll, self.serial_lines.len()),
        }
    }
}

/// Push a line into a buffer, trimming to MAX_LINES and adjusting scroll.
fn push_line(lines: &mut Vec<String>, scroll: &mut usize, line: String) {
    lines.push(line);
    if lines.len() > MAX_LINES {
        let excess = lines.len() - MAX_LINES;
        lines.drain(..excess);
        *scroll = scroll.saturating_sub(excess);
    }
}
