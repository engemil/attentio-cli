use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{Action, App};

/// Number of lines to scroll per PageUp/PageDown press.
const PAGE_SCROLL: usize = 10;

/// Handle a key event, updating app state and returning any action for the main loop.
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.running = false;
            Action::Quit
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.running = false;
            Action::Quit
        }
        KeyCode::PageUp => {
            app.scroll_up(PAGE_SCROLL);
            Action::None
        }
        KeyCode::PageDown => {
            app.scroll_down(PAGE_SCROLL);
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
        // 1-4: set runtime log level
        KeyCode::Char('1') => Action::SetLogLevel(1), // ERROR
        KeyCode::Char('2') => Action::SetLogLevel(2), // WARN
        KeyCode::Char('3') => Action::SetLogLevel(3), // INFO
        KeyCode::Char('4') => Action::SetLogLevel(4), // DEBUG
        _ => Action::None,
    }
}
