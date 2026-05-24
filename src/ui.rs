use crate::buffer::LineBuffer;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub struct UiState<'a> {
    pub port_name: &'a str,
    pub baud: u32,
    pub connected: bool,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub reconnect_enabled: bool,
    pub reconnect_attempt: u64,
    pub disconnect_count: u64,
    pub status: &'a str,
    pub log_path: Option<&'a str>,
    pub buffer: &'a LineBuffer,
    /// 0 means follow the tail; nonzero means scrolled up by N lines.
    pub scroll_offset: usize,
}

pub fn draw(frame: &mut Frame, state: &UiState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(1),    // body
            Constraint::Length(1), // hint bar
        ])
        .split(area);

    // Status bar.
    let conn_color = if state.connected {
        Color::Green
    } else {
        Color::Red
    };
    let conn_text = if state.connected { "OPEN" } else { "CLOSED" };
    let mut status_spans = vec![
        Span::styled(
            format!(" {} ", conn_text),
            Style::default()
                .fg(Color::Black)
                .bg(conn_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            state.port_name,
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{} bps", state.baud),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("   "),
        Span::styled(
            format!("rx {}  tx {}", state.bytes_rx, state.bytes_tx),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    if state.reconnect_enabled {
        status_spans.push(Span::raw("   "));
        if state.connected {
            status_spans.push(Span::styled(
                " RECONNECT READY ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            status_spans.push(Span::styled(
                format!(" RECONNECT #{} ", state.reconnect_attempt.max(1)),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        status_spans.push(Span::raw(" "));
        status_spans.push(Span::styled(
            format!("drops {}", state.disconnect_count),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if let Some(path) = state.log_path {
        status_spans.push(Span::raw("   "));
        status_spans.push(Span::styled(
            format!("log: {}", path),
            Style::default().fg(Color::Yellow),
        ));
    }
    if !state.status.is_empty() {
        status_spans.push(Span::raw("   "));
        status_spans.push(Span::styled(
            state.status,
            Style::default().fg(Color::Magenta),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(status_spans)), chunks[0]);

    // Body.
    let body_height = chunks[1].height.saturating_sub(2) as usize; // borders
    let total_lines = state.buffer.line_count();
    let visible = body_height.max(1);
    // If the user has scrolled up by `scroll_offset`, show the slice ending
    // at total_lines - scroll_offset.
    let end = total_lines.saturating_sub(state.scroll_offset);
    let start = end.saturating_sub(visible);
    let tail = state.buffer.tail(end);
    let slice: Vec<Line> = tail
        .iter()
        .skip(start.min(tail.len()))
        .map(|s| Line::from(Span::raw(*s)))
        .collect();

    let title = if state.scroll_offset > 0 {
        format!(
            " serial  ({} lines up — Esc to follow) ",
            state.scroll_offset
        )
    } else {
        " serial ".to_string()
    };

    let body = Paragraph::new(slice)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(body, chunks[1]);

    // Hint bar.
    let hint = Line::from(vec![
        Span::styled(" Ctrl+] ", Style::default().fg(Color::Cyan)),
        Span::raw("quit  "),
        Span::styled("Ctrl+B ", Style::default().fg(Color::Cyan)),
        Span::raw("send break  "),
        Span::styled("PgUp/PgDn ", Style::default().fg(Color::Cyan)),
        Span::raw("scroll  "),
        Span::styled("Esc ", Style::default().fg(Color::Cyan)),
        Span::raw("follow tail"),
    ]);
    frame.render_widget(Paragraph::new(hint), chunks[2]);
}
