use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{Action, App, Pane};

/// Number of lines to scroll per PageUp/PageDown press.
const PAGE_SCROLL: usize = 10;

/// Handle a key event, updating app state and returning any action for the main loop.
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Action {
    // Global keybindings (always active regardless of focus)
    match key.code {
        KeyCode::Esc => {
            app.running = false;
            return Action::Quit;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.running = false;
            return Action::Quit;
        }
        KeyCode::Tab => {
            app.toggle_focus();
            return Action::None;
        }
        KeyCode::PageUp => {
            app.scroll_up(PAGE_SCROLL);
            return Action::None;
        }
        KeyCode::PageDown => {
            app.scroll_down(PAGE_SCROLL);
            return Action::None;
        }
        _ => {}
    }

    // Focus-specific keybindings
    match app.focus {
        Pane::Shell => handle_shell_key(app, key),
        Pane::Debug => handle_debug_key(app, key),
    }
}

/// Handle key events when the shell pane is focused.
fn handle_shell_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Enter => {
            if let Some(cmd) = app.submit_input() {
                return Action::SendCommand(cmd);
            }
            Action::None
        }
        KeyCode::Char(ch) => {
            app.input_insert(ch);
            Action::None
        }
        KeyCode::Backspace => {
            app.input_backspace();
            Action::None
        }
        KeyCode::Delete => {
            app.input_delete();
            Action::None
        }
        KeyCode::Left => {
            app.input_left();
            Action::None
        }
        KeyCode::Right => {
            app.input_right();
            Action::None
        }
        KeyCode::Home => {
            app.input_home();
            Action::None
        }
        KeyCode::End => {
            app.input_end();
            Action::None
        }
        KeyCode::Up => {
            app.history_prev();
            Action::None
        }
        KeyCode::Down => {
            app.history_next();
            Action::None
        }
        _ => Action::None,
    }
}

/// Handle key events when the debug pane is focused.
fn handle_debug_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        // Allow typing even when debug pane is focused — forward to shell input
        KeyCode::Enter => {
            if let Some(cmd) = app.submit_input() {
                return Action::SendCommand(cmd);
            }
            Action::None
        }
        KeyCode::Char(ch) => {
            app.input_insert(ch);
            Action::None
        }
        KeyCode::Backspace => {
            app.input_backspace();
            Action::None
        }
        KeyCode::Up => {
            app.scroll_up(1);
            Action::None
        }
        KeyCode::Down => {
            app.scroll_down(1);
            Action::None
        }
        _ => Action::None,
    }
}
