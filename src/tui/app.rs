/// Maximum number of lines retained in the debug log buffer.
const MAX_DEBUG_LINES: usize = 1000;

/// Actions produced by event handling that the main loop must act on.
#[derive(Debug)]
pub enum Action {
    /// No external action needed.
    None,
    /// User requested quit.
    Quit,
}

/// Application state for the TUI.
pub struct App {
    /// CDC0 debug print lines (oldest first).
    pub debug_lines: Vec<String>,

    /// Scroll offset for the debug pane (0 = pinned to bottom / latest).
    /// Represents how many lines scrolled UP from the bottom.
    pub debug_scroll: usize,

    /// Whether the debug CDC port (CDC0) is connected.
    pub debug_connected: bool,

    /// Whether the debug port is currently attempting to reconnect.
    pub debug_reconnecting: bool,

    /// Whether the debug port is busy (held by another process).
    pub debug_port_busy: bool,

    /// Whether the TUI is still running.
    pub running: bool,

    /// Device serial number (for display).
    pub device_serial: String,
    /// CDC0 port path (for display).
    pub debug_port_path: Option<String>,
}

impl App {
    /// Create a new App with the given device info.
    pub fn new(
        device_serial: String,
        debug_port_path: Option<String>,
        debug_connected: bool,
    ) -> Self {
        Self {
            debug_lines: Vec::new(),
            debug_scroll: 0,
            debug_connected,
            debug_reconnecting: false,
            debug_port_busy: false,
            running: true,
            device_serial,
            debug_port_path,
        }
    }

    /// Append a line to the debug log buffer.
    pub fn push_debug_line(&mut self, line: String) {
        self.debug_lines.push(line);
        if self.debug_lines.len() > MAX_DEBUG_LINES {
            let excess = self.debug_lines.len() - MAX_DEBUG_LINES;
            self.debug_lines.drain(..excess);
            // Adjust scroll so the view doesn't jump
            self.debug_scroll = self.debug_scroll.saturating_sub(excess);
        }
    }

    // --- Scroll ---

    /// Scroll up by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        self.debug_scroll = self.debug_scroll.saturating_add(n);
        // Clamp to max scroll (total lines)
        self.debug_scroll = self.debug_scroll.min(self.debug_lines.len());
    }

    /// Scroll down by `n` lines (towards latest).
    pub fn scroll_down(&mut self, n: usize) {
        self.debug_scroll = self.debug_scroll.saturating_sub(n);
    }
}
