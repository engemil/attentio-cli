use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::app::App;

/// Render the TUI to the terminal frame.
/// Full-height debug pane with a status bar at the bottom.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(frame.area());

    render_debug_pane(frame, app, chunks[0]);
    render_status_bar(frame, app, chunks[1]);
}

/// Render the serial prints pane (CDC0).
fn render_debug_pane(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = Style::default().fg(Color::Cyan);

    let title = match (
        &app.debug_port_path,
        app.debug_connected,
        app.debug_reconnecting,
        app.debug_port_busy,
    ) {
        (Some(path), true, _, _) => format!(" Serial Prints (CDC0) \u{2014} {} ", path),
        (Some(path), false, _, true) => {
            format!(" Serial Prints (CDC0) \u{2014} {} (PORT BUSY) ", path)
        }
        (Some(path), false, true, _) => {
            format!(" Serial Prints (CDC0) \u{2014} {} (reconnecting...) ", path)
        }
        (Some(path), false, false, false) => {
            format!(" Serial Prints (CDC0) \u{2014} {} (not connected) ", path)
        }
        (None, _, _, _) => " Serial Prints (CDC0) \u{2014} (not connected) ".to_string(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !app.debug_connected {
        // Show centered status message
        let (text, color) = if app.debug_port_busy {
            ("(port busy \u{2014} close other process)", Color::Red)
        } else if app.debug_reconnecting {
            ("(reconnecting...)", Color::Yellow)
        } else {
            ("(not connected)", Color::DarkGray)
        };
        let msg = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))))
            .alignment(Alignment::Center);
        // Center vertically
        let y_offset = inner.height / 2;
        let centered = Rect::new(inner.x, inner.y + y_offset, inner.width, 1);
        frame.render_widget(msg, centered);
        return;
    }

    // Calculate visible lines with scroll offset
    let visible_height = inner.height as usize;
    let total_lines = app.debug_lines.len();

    let lines = build_scrolled_lines(&app.debug_lines, app.debug_scroll, visible_height);

    // Show scroll indicator if not at bottom
    let paragraph = if app.debug_scroll > 0 {
        let scroll_indicator = format!(
            " \u{2191} {} more below ",
            app.debug_scroll.min(total_lines)
        );
        let mut display_lines = lines;
        if !display_lines.is_empty() {
            let last_idx = display_lines.len() - 1;
            display_lines[last_idx] = Line::from(Span::styled(
                scroll_indicator,
                Style::default().fg(Color::Yellow),
            ));
        }
        Paragraph::new(display_lines).wrap(Wrap { trim: false })
    } else {
        Paragraph::new(lines).wrap(Wrap { trim: false })
    };

    frame.render_widget(paragraph, inner);
}

/// Render the bottom status bar with device info and key hints.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut hints = vec![
        Span::styled(" ", Style::default()),
        Span::styled(
            format!("[{}] ", app.device_serial),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    // Show current runtime log level
    {
        let (label, color) = match app.log_level {
            Some(0) => ("Log:NONE ", Color::DarkGray),
            Some(1) => ("Log:ERROR ", Color::Red),
            Some(2) => ("Log:WARN ", Color::Yellow),
            Some(3) => ("Log:INFO ", Color::Green),
            Some(4) => ("Log:DEBUG ", Color::Magenta),
            _ => ("Log:? ", Color::DarkGray),
        };
        hints.push(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }

    hints.extend([
        Span::styled("1-4", Style::default().fg(Color::DarkGray)),
        Span::styled("=loglevel ", Style::default().fg(Color::DarkGray)),
        Span::styled("| PgUp/PgDn", Style::default().fg(Color::DarkGray)),
        Span::styled("=scroll ", Style::default().fg(Color::DarkGray)),
        Span::styled("| Esc", Style::default().fg(Color::DarkGray)),
        Span::styled("=quit ", Style::default().fg(Color::DarkGray)),
    ]);

    let status_line = Line::from(hints);
    let status_bar =
        Paragraph::new(status_line).style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(status_bar, area);
}

/// Build a vec of Lines from a string buffer, applying scroll offset.
///
/// `scroll_offset` is how many lines from the bottom are hidden (0 = pinned to bottom).
fn build_scrolled_lines<'a>(
    lines: &'a [String],
    scroll_offset: usize,
    visible_height: usize,
) -> Vec<Line<'a>> {
    let total = lines.len();
    if total == 0 {
        return Vec::new();
    }

    // Calculate the range of lines to show
    // end is exclusive, and represents the last line we'd show at the bottom
    let end = total.saturating_sub(scroll_offset);
    let start = end.saturating_sub(visible_height);

    lines[start..end]
        .iter()
        .map(|s| Line::from(s.as_str()))
        .collect()
}
