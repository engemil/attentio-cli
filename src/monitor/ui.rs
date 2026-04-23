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

    render_pane(
        frame,
        PaneView {
            area: chunks[0],
            focused: app.active_pane == Pane::Protocol,
            title_prefix: "AP Protocol (CDC1)",
            no_port_title: "(no port)",
            port_path: app.ap_port_path.as_deref(),
            connected: app.ap_connected,
            reconnecting: app.ap_reconnecting,
            port_busy: app.ap_port_busy,
            show_disconnected_without_port: false,
            disconnected_text_when_idle: "(not connected)",
            waiting_text: Some("(waiting for AP traffic...)"),
            lines: &app.ap_lines,
            scroll: app.ap_scroll,
        },
    );
    render_pane(
        frame,
        PaneView {
            area: chunks[1],
            focused: app.active_pane == Pane::Serial,
            title_prefix: "Serial Prints (CDC0)",
            no_port_title: "(not connected)",
            port_path: app.serial_port_path.as_deref(),
            connected: app.serial_connected,
            reconnecting: app.serial_reconnecting,
            port_busy: app.serial_port_busy,
            show_disconnected_without_port: true,
            disconnected_text_when_idle: "(not connected)",
            waiting_text: None,
            lines: &app.serial_lines,
            scroll: app.serial_scroll,
        },
    );
    render_status_bar(frame, app, chunks[2]);
}

struct PaneView<'a> {
    area: Rect,
    focused: bool,
    title_prefix: &'a str,
    no_port_title: &'a str,
    port_path: Option<&'a str>,
    connected: bool,
    reconnecting: bool,
    port_busy: bool,
    show_disconnected_without_port: bool,
    disconnected_text_when_idle: &'a str,
    waiting_text: Option<&'a str>,
    lines: &'a [String],
    scroll: usize,
}

fn render_pane(frame: &mut Frame, pane: PaneView<'_>) {
    let border_color = if pane.focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let title = match (
        pane.port_path,
        pane.connected,
        pane.reconnecting,
        pane.port_busy,
    ) {
        (Some(path), true, _, _) => format!(" {} — {} ", pane.title_prefix, path),
        (Some(path), false, _, true) => {
            format!(" {} — {} (PORT BUSY) ", pane.title_prefix, path)
        }
        (Some(path), false, true, _) => {
            format!(" {} — {} (reconnecting...) ", pane.title_prefix, path)
        }
        (Some(path), false, false, false) => {
            format!(" {} — {} (not connected) ", pane.title_prefix, path)
        }
        (None, _, _, _) => format!(" {} — {} ", pane.title_prefix, pane.no_port_title),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(pane.area);
    frame.render_widget(block, pane.area);

    if !pane.connected && (pane.show_disconnected_without_port || pane.port_path.is_some()) {
        let (text, color) = if pane.port_busy {
            ("(port busy — close other process)", Color::Red)
        } else if pane.reconnecting {
            ("(reconnecting...)", Color::Yellow)
        } else {
            (pane.disconnected_text_when_idle, Color::DarkGray)
        };
        let msg = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))))
            .alignment(Alignment::Center);
        let y_offset = inner.height / 2;
        let centered = Rect::new(inner.x, inner.y + y_offset, inner.width, 1);
        frame.render_widget(msg, centered);
        return;
    }

    if let Some(waiting_text) = pane.waiting_text.filter(|_| pane.lines.is_empty()) {
        let msg = Paragraph::new(Line::from(Span::styled(
            waiting_text,
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center);
        let y_offset = inner.height / 2;
        let centered = Rect::new(inner.x, inner.y + y_offset, inner.width, 1);
        frame.render_widget(msg, centered);
        return;
    }

    render_scrolled_content(frame, pane.lines, pane.scroll, inner);
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
