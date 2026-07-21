use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::App;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    draw_title(frame, app, chunks[0]);

    if app.shortcuts.is_empty() {
        draw_empty_state(frame, app, chunks[1]);
    } else {
        draw_table(frame, app, chunks[1]);
    }

    draw_footer(frame, app, chunks[2]);
}

fn draw_title(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = Paragraph::new(Line::from(format!(
        "CachyCuts — {} shortcuts from {}",
        app.shortcuts.len(),
        app.source_path.display()
    )))
    .style(Style::default().fg(Color::Cyan))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, area);
}

fn draw_empty_state(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let message = app
        .error
        .as_deref()
        .unwrap_or("No shortcuts to display.");
    let body = Paragraph::new(message)
        .style(Style::default().fg(Color::Red))
        .block(Block::default().borders(Borders::ALL).title("Shortcuts"));
    frame.render_widget(body, area);
}

fn draw_table(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let header = Row::new(vec![
        Cell::from("Keys"),
        Cell::from("Action"),
        Cell::from("Description"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows = app.shortcuts.iter().map(|shortcut| {
        Row::new(vec![
            Cell::from(shortcut.key_combo()),
            Cell::from(shortcut.action()),
            Cell::from(shortcut.label().to_string()),
        ])
    });

    let widths = [
        Constraint::Length(22),
        Constraint::Percentage(40),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Shortcuts"))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");

    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_footer(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let text = if let Some(error) = &app.error {
        Line::from(error.as_str()).style(Style::default().fg(Color::Red))
    } else {
        Line::from("↑/k ↓/j move   g/G top/bottom   q quit")
    };
    frame.render_widget(Paragraph::new(text), area);
}
