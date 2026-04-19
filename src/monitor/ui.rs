use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::app::{App, Pane};

/// Render the TUI to the terminal frame.
/// Two-pane layout: AP protocol traffic (top), serial prints (bottom),
/// plus a status bar.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35), // AP protocol pane (top)
            Constraint::Min(5),         // Serial prints pane (bottom)
            Constraint::Length(1),      // Status bar
        ])
        .split(frame.area());

    render_ap_pane(frame, app, chunks[0]);
    render_serial_pane(frame, app, chunks[1]);
    render_status_bar(frame, app, chunks[2]);
}

/// Render the AP protocol traffic pane (CDC1) — top pane.
fn render_ap_pane(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.active_pane == Pane::Protocol;
    let border_color = if focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let title = match (
        &app.ap_port_path,
        app.ap_connected,
        app.ap_reconnecting,
        app.ap_port_busy,
    ) {
        (Some(path), true, _, _) => format!(" AP Protocol (CDC1) — {} ", path),
        (Some(path), false, _, true) => {
            format!(" AP Protocol (CDC1) — {} (PORT BUSY) ", path)
        }
        (Some(path), false, true, _) => {
            format!(" AP Protocol (CDC1) — {} (reconnecting...) ", path)
        }
        (Some(path), false, false, false) => {
            format!(" AP Protocol (CDC1) — {} (not connected) ", path)
        }
        (None, _, _, _) => " AP Protocol (CDC1) — (no port) ".to_string(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !app.ap_connected && app.ap_port_path.is_some() {
        let (text, color) = if app.ap_port_busy {
            ("(port busy — close other process)", Color::Red)
        } else if app.ap_reconnecting {
            ("(reconnecting...)", Color::Yellow)
        } else {
            ("(not connected)", Color::DarkGray)
        };
        let msg = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))))
            .alignment(Alignment::Center);
        let y_offset = inner.height / 2;
        let centered = Rect::new(inner.x, inner.y + y_offset, inner.width, 1);
        frame.render_widget(msg, centered);
        return;
    }

    if app.ap_lines.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "(waiting for AP traffic...)",
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center);
        let y_offset = inner.height / 2;
        let centered = Rect::new(inner.x, inner.y + y_offset, inner.width, 1);
        frame.render_widget(msg, centered);
        return;
    }

    render_scrolled_content(frame, &app.ap_lines, app.ap_scroll, inner);
}

/// Render the serial prints pane (CDC0) — bottom pane.
fn render_serial_pane(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.active_pane == Pane::Serial;
    let border_color = if focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let title = match (
        &app.serial_port_path,
        app.serial_connected,
        app.serial_reconnecting,
        app.serial_port_busy,
    ) {
        (Some(path), true, _, _) => format!(" Serial Prints (CDC0) — {} ", path),
        (Some(path), false, _, true) => {
            format!(" Serial Prints (CDC0) — {} (PORT BUSY) ", path)
        }
        (Some(path), false, true, _) => {
            format!(" Serial Prints (CDC0) — {} (reconnecting...) ", path)
        }
        (Some(path), false, false, false) => {
            format!(" Serial Prints (CDC0) — {} (not connected) ", path)
        }
        (None, _, _, _) => " Serial Prints (CDC0) — (not connected) ".to_string(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !app.serial_connected {
        let (text, color) = if app.serial_port_busy {
            ("(port busy — close other process)", Color::Red)
        } else if app.serial_reconnecting {
            ("(reconnecting...)", Color::Yellow)
        } else {
            ("(not connected)", Color::DarkGray)
        };
        let msg = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))))
            .alignment(Alignment::Center);
        let y_offset = inner.height / 2;
        let centered = Rect::new(inner.x, inner.y + y_offset, inner.width, 1);
        frame.render_widget(msg, centered);
        return;
    }

    render_scrolled_content(frame, &app.serial_lines, app.serial_scroll, inner);
}

/// Render a scrollable text buffer into a given area.
fn render_scrolled_content(frame: &mut Frame, lines: &[String], scroll_offset: usize, area: Rect) {
    let visible_height = area.height as usize;
    let total = lines.len();

    let display_lines = build_scrolled_lines(lines, scroll_offset, visible_height);

    let paragraph = if scroll_offset > 0 {
        let scroll_indicator = format!(" ↑ {} more below ", scroll_offset.min(total));
        let mut display = display_lines;
        if !display.is_empty() {
            let last_idx = display.len() - 1;
            display[last_idx] = Line::from(Span::styled(
                scroll_indicator,
                Style::default().fg(Color::Yellow),
            ));
        }
        Paragraph::new(display).wrap(Wrap { trim: false })
    } else {
        Paragraph::new(display_lines).wrap(Wrap { trim: false })
    };

    frame.render_widget(paragraph, area);
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
            Some(level @ 0..=4) => {
                let name = crate::cli::commands::loglevel::level_name(level);
                let color = match level {
                    0 => Color::DarkGray,
                    1 => Color::Red,
                    2 => Color::Yellow,
                    3 => Color::Green,
                    4 => Color::Magenta,
                    _ => Color::DarkGray,
                };
                (format!("Log:{} ", name), color)
            }
            _ => ("Log:? ".to_string(), Color::DarkGray),
        };
        hints.push(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }

    // Show active pane indicator
    let pane_label = match app.active_pane {
        Pane::Protocol => "Focus:AP ",
        Pane::Serial => "Focus:SER ",
    };
    hints.push(Span::styled(
        pane_label,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));

    hints.extend([
        Span::styled("Tab", Style::default().fg(Color::DarkGray)),
        Span::styled("=pane ", Style::default().fg(Color::DarkGray)),
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

    let end = total.saturating_sub(scroll_offset);
    let start = end.saturating_sub(visible_height);

    lines[start..end]
        .iter()
        .map(|s| Line::from(s.as_str()))
        .collect()
}
