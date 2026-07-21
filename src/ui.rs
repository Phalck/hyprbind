use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::{App, Mode};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let footer_height = match app.mode {
        Mode::EditKey | Mode::EditTarget | Mode::EditMainMod => 2,
        Mode::Normal | Mode::Search => 1,
    };
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(footer_height),
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
    let lines: Vec<Line> = match app.mode {
        Mode::EditKey => edit_footer_lines("key", app),
        Mode::EditTarget => edit_footer_lines("target", app),
        Mode::EditMainMod => edit_footer_lines("$mainMod", app),
        Mode::Search => vec![Line::from(format!(
            "/{}▏   Enter apply   Esc cancel",
            app.query
        ))],
        Mode::Normal => {
            let line = if let Some(error) = &app.error {
                Line::from(error.as_str()).style(Style::default().fg(Color::Red))
            } else if let Some(status) = &app.status {
                Line::from(status.as_str()).style(Style::default().fg(Color::Green))
            } else if app.query.is_empty() {
                Line::from(
                    "/ search   e edit key   t edit target   E edit $mainMod   ↑/k ↓/j move   g/G top/bottom   q quit",
                )
            } else {
                Line::from(format!(
                    "filter: \"{}\"   / change filter   e edit key   t edit target   E edit $mainMod   ↑/k ↓/j move   g/G top/bottom   q quit",
                    app.query
                ))
            };
            vec![line]
        }
    };
    frame.render_widget(Paragraph::new(lines), area);
}

fn edit_footer_lines(field: &str, app: &App) -> Vec<Line<'static>> {
    let line_no = app.editing_line.unwrap_or(0);
    let cursor_byte = app.edit_cursor_byte_offset();
    let before = app.edit_buffer[..cursor_byte].to_string();
    let mut rest = app.edit_buffer[cursor_byte..].chars();
    // Highlight the character the cursor sits on, like a terminal block cursor, instead of
    // inserting a separate cursor glyph that would shift everything after it over by a cell.
    let under_cursor = rest.next().unwrap_or(' ').to_string();
    let after: String = rest.collect();

    let prompt = Line::from(vec![
        Span::raw(format!("editing {field} (line {line_no}): {before}")),
        Span::styled(under_cursor, Style::default().add_modifier(Modifier::REVERSED)),
        Span::raw(after),
    ]);

    vec![prompt, Line::from("Enter save   Esc cancel   ←/→ move   Home/End jump")]
}
