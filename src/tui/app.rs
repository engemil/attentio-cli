/// Maximum number of lines retained in the debug log buffer.
const MAX_DEBUG_LINES: usize = 1000;

/// Maximum number of lines retained in the shell output buffer.
const MAX_SHELL_LINES: usize = 1000;

/// Maximum number of commands retained in history.
const MAX_COMMAND_HISTORY: usize = 100;

/// Which pane currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Debug,
    Shell,
}

/// Actions produced by event handling that the main loop must act on.
#[derive(Debug)]
pub enum Action {
    /// No external action needed.
    None,
    /// Send this command string to CDC1.
    SendCommand(String),
    /// User requested quit.
    Quit,
}

/// Application state for the TUI.
pub struct App {
    /// CDC0 debug print lines (oldest first).
    pub debug_lines: Vec<String>,
    /// CDC1 shell output lines (oldest first).
    pub shell_lines: Vec<String>,

    /// Current text being typed in the input line.
    pub input: String,
    /// Cursor position within `input` (byte offset, but we only support ASCII-safe insertion).
    pub input_cursor: usize,

    /// Scroll offset for the debug pane (0 = pinned to bottom / latest).
    /// Represents how many lines scrolled UP from the bottom.
    pub debug_scroll: usize,
    /// Scroll offset for the shell pane.
    pub shell_scroll: usize,

    /// Which pane has keyboard focus.
    pub focus: Pane,

    /// Whether the debug CDC port (CDC0) is connected.
    pub debug_connected: bool,
    /// Whether the shell CDC port (CDC1) is connected.
    pub shell_connected: bool,

    /// Whether the debug port is currently attempting to reconnect.
    pub debug_reconnecting: bool,
    /// Whether the shell port is currently attempting to reconnect.
    pub shell_reconnecting: bool,

    /// Whether the debug port is busy (held by another process).
    pub debug_port_busy: bool,
    /// Whether the shell port is busy (held by another process).
    pub shell_port_busy: bool,

    /// Whether the TUI is still running.
    pub running: bool,

    /// Device serial number (for display).
    pub device_serial: String,
    /// CDC0 port path (for display).
    pub debug_port_path: Option<String>,
    /// CDC1 / shell port path (for display).
    pub shell_port_path: Option<String>,

    /// Previously sent commands for up/down recall.
    command_history: Vec<String>,
    /// Current position in command history (None = not browsing history).
    history_index: Option<usize>,
    /// Saved input text when entering history browsing.
    history_saved_input: String,
}

impl App {
    /// Create a new App with the given device info.
    pub fn new(
        device_serial: String,
        debug_port_path: Option<String>,
        shell_port_path: Option<String>,
        debug_connected: bool,
        shell_connected: bool,
    ) -> Self {
        Self {
            debug_lines: Vec::new(),
            shell_lines: Vec::new(),
            input: String::new(),
            input_cursor: 0,
            debug_scroll: 0,
            shell_scroll: 0,
            focus: Pane::Shell,
            debug_connected,
            shell_connected,
            debug_reconnecting: false,
            shell_reconnecting: false,
            debug_port_busy: false,
            shell_port_busy: false,
            running: true,
            device_serial,
            debug_port_path,
            shell_port_path,
            command_history: Vec::new(),
            history_index: None,
            history_saved_input: String::new(),
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

    /// Append a line to the shell output buffer.
    pub fn push_shell_line(&mut self, line: String) {
        self.shell_lines.push(line);
        if self.shell_lines.len() > MAX_SHELL_LINES {
            let excess = self.shell_lines.len() - MAX_SHELL_LINES;
            self.shell_lines.drain(..excess);
            self.shell_scroll = self.shell_scroll.saturating_sub(excess);
        }
    }

    /// Submit the current input as a command. Returns the command string if non-empty.
    /// Returns None if input is empty or shell is not connected.
    pub fn submit_input(&mut self) -> Option<String> {
        let cmd = self.input.trim().to_string();
        if cmd.is_empty() {
            return None;
        }

        if !self.shell_connected {
            // Show the attempted command and error in shell output
            self.push_shell_line(format!("> {}", cmd));
            self.push_shell_line("(shell not connected)".to_string());
            self.input.clear();
            self.input_cursor = 0;
            self.shell_scroll = 0;
            return None;
        }

        // Add to command history (avoid consecutive duplicates)
        if self.command_history.last().map(|s| s.as_str()) != Some(&cmd) {
            self.command_history.push(cmd.clone());
            if self.command_history.len() > MAX_COMMAND_HISTORY {
                self.command_history.remove(0);
            }
        }

        // Show the command in shell output
        self.push_shell_line(format!("> {}", cmd));

        // Reset input state
        self.input.clear();
        self.input_cursor = 0;
        self.history_index = None;
        self.history_saved_input.clear();

        // Pin shell scroll to bottom when sending a command
        self.shell_scroll = 0;

        Some(cmd)
    }

    // --- Input editing ---

    /// Insert a character at the cursor position.
    pub fn input_insert(&mut self, ch: char) {
        self.input.insert(self.input_cursor, ch);
        self.input_cursor += ch.len_utf8();
    }

    /// Delete the character before the cursor (backspace).
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            // Find the previous character boundary
            let prev = self.input[..self.input_cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.remove(prev);
            self.input_cursor = prev;
        }
    }

    /// Delete the character at the cursor (delete key).
    pub fn input_delete(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input.remove(self.input_cursor);
        }
    }

    /// Move cursor left by one character.
    pub fn input_left(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor = self.input[..self.input_cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right by one character.
    pub fn input_right(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input_cursor = self.input[self.input_cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.input_cursor + i)
                .unwrap_or(self.input.len());
        }
    }

    /// Move cursor to the beginning of input.
    pub fn input_home(&mut self) {
        self.input_cursor = 0;
    }

    /// Move cursor to the end of input.
    pub fn input_end(&mut self) {
        self.input_cursor = self.input.len();
    }

    // --- Command history navigation ---

    /// Navigate to the previous command in history (up arrow).
    pub fn history_prev(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // Entering history mode — save current input
                self.history_saved_input = self.input.clone();
                let idx = self.command_history.len() - 1;
                self.history_index = Some(idx);
                self.input = self.command_history[idx].clone();
                self.input_cursor = self.input.len();
            }
            Some(idx) if idx > 0 => {
                let new_idx = idx - 1;
                self.history_index = Some(new_idx);
                self.input = self.command_history[new_idx].clone();
                self.input_cursor = self.input.len();
            }
            _ => {} // Already at oldest entry
        }
    }

    /// Navigate to the next command in history (down arrow).
    pub fn history_next(&mut self) {
        if let Some(idx) = self.history_index {
            if idx + 1 < self.command_history.len() {
                let new_idx = idx + 1;
                self.history_index = Some(new_idx);
                self.input = self.command_history[new_idx].clone();
                self.input_cursor = self.input.len();
            } else {
                // Past the newest entry — restore saved input
                self.history_index = None;
                self.input = self.history_saved_input.clone();
                self.input_cursor = self.input.len();
                self.history_saved_input.clear();
            }
        }
    }

    // --- Scroll ---

    /// Scroll the focused pane up by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        match self.focus {
            Pane::Debug => {
                self.debug_scroll = self.debug_scroll.saturating_add(n);
                // Clamp to max scroll (total lines)
                self.debug_scroll = self.debug_scroll.min(self.debug_lines.len());
            }
            Pane::Shell => {
                self.shell_scroll = self.shell_scroll.saturating_add(n);
                self.shell_scroll = self.shell_scroll.min(self.shell_lines.len());
            }
        }
    }

    /// Scroll the focused pane down by `n` lines (towards latest).
    pub fn scroll_down(&mut self, n: usize) {
        match self.focus {
            Pane::Debug => {
                self.debug_scroll = self.debug_scroll.saturating_sub(n);
            }
            Pane::Shell => {
                self.shell_scroll = self.shell_scroll.saturating_sub(n);
            }
        }
    }

    /// Toggle focus between debug and shell panes.
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Pane::Debug => Pane::Shell,
            Pane::Shell => Pane::Debug,
        };
    }
}
