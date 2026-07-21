use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::{App, Mode};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    draw_title(frame, app, chunks[0]);

    let visible_len = app.visible().len();
    if app.shortcuts.is_empty() {
        draw_message(frame, chunks[1], app.error.as_deref().unwrap_or("No shortcuts to display."));
    } else if visible_len == 0 {
        draw_message(
            frame,
            chunks[1],
            &format!("No shortcuts match \"{}\".", app.query),
        );
    } else {
        draw_table(frame, app, chunks[1]);
    }

    draw_footer(frame, app, chunks[2]);
}

fn draw_title(frame: &mut Frame, app: &App, area: Rect) {
    let total = app.shortcuts.len();
    let count_text = if app.query.is_empty() {
        format!("{total} shortcuts")
    } else {
        format!("{} of {total} shortcuts", app.visible().len())
    };
    let title = Paragraph::new(Line::from(format!(
        "CachyCuts — {count_text} from {}",
        app.source_path.display()
    )))
    .style(Style::default().fg(Color::Cyan))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, area);
}

fn draw_message(frame: &mut Frame, area: Rect, message: &str) {
    let body = Paragraph::new(message)
        .style(Style::default().fg(Color::Red))
        .block(Block::default().borders(Borders::ALL).title("Shortcuts"));
    frame.render_widget(body, area);
}

fn draw_table(frame: &mut Frame, app: &mut App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Keys"),
        Cell::from("Action"),
        Cell::from("Description"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .visible()
        .iter()
        .map(|shortcut| {
            Row::new(vec![
                Cell::from(shortcut.key_combo()),
                Cell::from(shortcut.action()),
                Cell::from(shortcut.label().to_string()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(22),
        Constraint::Percentage(40),
        Constraint::Min(20),
    ];

    let title = if app.query.is_empty() {
        "Shortcuts".to_string()
    } else {
        format!("Shortcuts (filter: \"{}\")", app.query)
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");

    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let text = match app.mode {
        Mode::Search => Line::from(format!("/{}▏   Enter apply   Esc cancel", app.query)),
        Mode::Normal => {
            if let Some(error) = &app.error {
                Line::from(error.as_str()).style(Style::default().fg(Color::Red))
            } else if app.query.is_empty() {
                Line::from("/ search   ↑/k ↓/j move   g/G top/bottom   q quit")
            } else {
                Line::from(format!(
                    "filter: \"{}\"   / edit   ↑/k ↓/j move   g/G top/bottom   q quit",
                    app.query
                ))
            }
        }
    };
    frame.render_widget(Paragraph::new(text), area);
}
