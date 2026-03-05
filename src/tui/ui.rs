use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::app::{App, Pane};

/// Render the TUI to the terminal frame.
/// Always renders both panes (debug on top, shell on bottom).
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(frame.area());

    render_debug_pane(frame, app, chunks[0]);
    render_shell_pane(frame, app, chunks[1]);
}

/// Render the debug prints pane (CDC0).
fn render_debug_pane(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Pane::Debug;

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = match (&app.debug_port_path, app.debug_connected) {
        (Some(path), true) => format!(" Debug Prints (CDC0) \u{2014} {} ", path),
        (Some(path), false) => format!(" Debug Prints (CDC0) \u{2014} {} (not connected) ", path),
        (None, _) => " Debug Prints (CDC0) \u{2014} (not connected) ".to_string(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !app.debug_connected {
        // Show centered "(not connected)" message
        let msg = Paragraph::new(Line::from(Span::styled(
            "(not connected)",
            Style::default().fg(Color::DarkGray),
        )))
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

/// Render the shell pane (CDC1): output area + input line at bottom.
fn render_shell_pane(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Pane::Shell;

    // Split the shell area: output on top, input line at bottom, status bar at very bottom
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // Shell output
            Constraint::Length(3), // Input line
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    // --- Shell output ---
    let border_style = if is_focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = match (&app.shell_port_path, app.shell_connected) {
        (Some(path), true) => format!(" Shell (CDC1) \u{2014} {} ", path),
        (Some(path), false) => format!(" Shell (CDC1) \u{2014} {} (not connected) ", path),
        (None, _) => " Shell (CDC1) \u{2014} (not connected) ".to_string(),
    };

    let output_block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let output_inner = output_block.inner(chunks[0]);
    frame.render_widget(output_block, chunks[0]);

    if !app.shell_connected {
        // Show centered "(not connected)" message
        let msg = Paragraph::new(Line::from(Span::styled(
            "(not connected)",
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center);
        let y_offset = output_inner.height / 2;
        let centered = Rect::new(
            output_inner.x,
            output_inner.y + y_offset,
            output_inner.width,
            1,
        );
        frame.render_widget(msg, centered);
    } else {
        let visible_height = output_inner.height as usize;
        let lines = build_scrolled_lines(&app.shell_lines, app.shell_scroll, visible_height);

        let output_paragraph = if app.shell_scroll > 0 {
            let total_lines = app.shell_lines.len();
            let scroll_indicator = format!(
                " \u{2191} {} more below ",
                app.shell_scroll.min(total_lines)
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

        frame.render_widget(output_paragraph, output_inner);
    }

    // --- Input line ---
    let input_style = if app.shell_connected {
        if is_focused {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let input_block = Block::default()
        .title(" Command ")
        .borders(Borders::ALL)
        .border_style(input_style);

    let input_inner = input_block.inner(chunks[1]);
    frame.render_widget(input_block, chunks[1]);

    // Build the input line with prompt
    let prompt_color = if app.shell_connected {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let text_color = if app.shell_connected {
        Color::Reset
    } else {
        Color::DarkGray
    };

    let prompt = Span::styled("attentio> ", Style::default().fg(prompt_color));
    let input_text = Span::styled(&app.input, Style::default().fg(text_color));
    let input_line = Line::from(vec![prompt, input_text]);
    let input_paragraph = Paragraph::new(input_line);

    frame.render_widget(input_paragraph, input_inner);

    // Position the cursor in the input field
    if is_focused && app.shell_connected {
        let prompt_len = "attentio> ".len() as u16;
        let cursor_x = input_inner.x + prompt_len + app.input_cursor as u16;
        let cursor_y = input_inner.y;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    // --- Status bar ---
    render_status_bar(frame, app, chunks[2]);
}

/// Render the bottom status bar with device info and key hints.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let focus_name = match app.focus {
        Pane::Debug => "Debug",
        Pane::Shell => "Shell",
    };

    let hints = vec![
        Span::styled(" ", Style::default()),
        Span::styled(
            format!("[{}] ", app.device_serial),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("Focus: {} ", focus_name),
            Style::default().fg(Color::White),
        ),
        Span::styled("| Tab", Style::default().fg(Color::DarkGray)),
        Span::styled("=switch ", Style::default().fg(Color::DarkGray)),
        Span::styled("| PgUp/PgDn", Style::default().fg(Color::DarkGray)),
        Span::styled("=scroll ", Style::default().fg(Color::DarkGray)),
        Span::styled("| Esc", Style::default().fg(Color::DarkGray)),
        Span::styled("=quit ", Style::default().fg(Color::DarkGray)),
    ];

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
