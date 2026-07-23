use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::{App, Mode};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let footer_lines = footer_lines(app, area.width);
    let footer_height = footer_lines.len() as u16;
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(0),
        Constraint::Length(footer_height),
    ])
    .split(area);

    draw_title(frame, app, chunks[0]);

    match app.mode {
        Mode::TemplateSaveSelect | Mode::TemplatePreview => draw_template_picker(frame, app, chunks[1]),
        Mode::TemplateList => draw_template_list(frame, app, chunks[1]),
        Mode::BackupList => draw_backup_list(frame, app, chunks[1]),
        Mode::BackupConfirm => draw_backup_confirm(frame, app, chunks[1]),
        Mode::DuplicateKeyConfirm => draw_duplicate_confirm(frame, app, chunks[1]),
        _ => {
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
        }
    }

    frame.render_widget(Paragraph::new(footer_lines), chunks[2]);
}

fn draw_title(frame: &mut Frame, app: &App, area: Rect) {
    let total = app.shortcuts.len();
    let count_text = if app.query.is_empty() {
        format!("{total} shortcuts")
    } else {
        format!("{} of {total} shortcuts", app.visible().len())
    };
    let (name, tooltip) = mode_help(app.mode);

    let lines = vec![
        Line::from(format!(
            "hyprbind — {count_text} from {}",
            app.source_path.display()
        ))
        .style(Style::default().fg(Color::Cyan)),
        Line::from(vec![
            Span::styled(name, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!(": {tooltip}")),
        ]),
    ];

    let title = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, area);
}

/// Display name and one-line description of the command the user currently has selected (i.e.
/// the active mode), shown in the header.
fn mode_help(mode: Mode) -> (&'static str, &'static str) {
    match mode {
        Mode::Normal => (
            "Browse",
            "select a shortcut, then e/a/d/E to edit, A to add one, o to open its script's folder, or t/l for templates",
        ),
        Mode::Search => (
            "Search",
            "type to filter shortcuts live; Enter applies, Esc clears",
        ),
        Mode::EditKey => (
            "Edit key",
            "change the modifiers and key of the selected shortcut",
        ),
        Mode::EditTarget => (
            "Edit target",
            "change the dispatcher and arguments of the selected shortcut",
        ),
        Mode::EditDescription => (
            "Edit description",
            "change the description of the selected shortcut",
        ),
        Mode::EditMainMod => (
            "Edit $mainMod",
            "change the value every shortcut's $mainMod reference resolves to",
        ),
        Mode::AddShortcut => (
            "Add shortcut",
            "type a description; Enter creates the shortcut, then e/a set its key and target",
        ),
        Mode::SourcePath => (
            "Keybindings file",
            "set the path to the Hyprland config file to read and edit",
        ),
        Mode::TemplateFolder => (
            "Template folder",
            "set where templates are saved to and loaded from",
        ),
        Mode::TemplateSaveSelect => (
            "Save template",
            "Space to check shortcuts, Enter to name and save them",
        ),
        Mode::TemplateSaveName => (
            "Name template",
            "type a file name; Enter writes the checked shortcuts",
        ),
        Mode::TemplateList => (
            "Load template",
            "pick a .hbt file to preview the shortcuts in it",
        ),
        Mode::TemplatePreview => (
            "Apply template",
            "Space to toggle, Enter applies the checked shortcuts",
        ),
        Mode::BackupFolder => (
            "Backup folder",
            "set where backups are saved to and restored from",
        ),
        Mode::BackupList => ("Restore backup", "pick a .hbb file to restore"),
        Mode::TerminalCommand => (
            "Terminal command",
            "set the program (and any fixed arguments) used to open a terminal",
        ),
        Mode::BackupConfirm => (
            "Confirm restore",
            "overwrite the current keybindings file with this backup",
        ),
        Mode::DuplicateKeyConfirm => (
            "Key already used",
            "add the suggested modifier, or cancel to keep the original key",
        ),
    }
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

/// The checkbox multi-select list shared by "pick rows to save" and "pick rows to apply".
fn draw_template_picker(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.template_candidates.is_empty() {
        let message = match app.mode {
            Mode::TemplateSaveSelect => "No shortcuts to save (the current list is empty).",
            _ => "No shortcuts found in this template.",
        };
        draw_message(frame, area, message);
        return;
    }

    let title = match app.mode {
        Mode::TemplateSaveSelect => "Select shortcuts to save".to_string(),
        _ => {
            let name = app.template_source_name.as_deref().unwrap_or("template");
            format!("Select shortcuts to apply from {name}")
        }
    };

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Keys"),
        Cell::from("Action"),
        Cell::from("Description"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .template_candidates
        .iter()
        .map(|shortcut| {
            let checked = if app.template_selected.contains(&shortcut.line) {
                "[x]"
            } else {
                "[ ]"
            };
            Row::new(vec![
                Cell::from(checked),
                Cell::from(shortcut.key_combo()),
                Cell::from(shortcut.action()),
                Cell::from(shortcut.label().to_string()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(3),
        Constraint::Length(22),
        Constraint::Percentage(38),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");

    frame.render_stateful_widget(table, area, &mut app.template_table_state);
}

fn draw_template_list(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.template_files.is_empty() {
        draw_message(
            frame,
            area,
            &format!("No .hbt templates found in {}.", app.template_folder.display()),
        );
        return;
    }

    let rows: Vec<Row> = app
        .template_files
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Row::new(vec![Cell::from(name)])
        })
        .collect();

    let widths = [Constraint::Percentage(100)];
    let title = format!("Templates in {}", app.template_folder.display());

    let table = Table::new(rows, widths)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");

    frame.render_stateful_widget(table, area, &mut app.template_table_state);
}

fn draw_backup_list(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.backup_files.is_empty() {
        draw_message(
            frame,
            area,
            &format!("No .hbb backups found in {}.", app.backup_folder.display()),
        );
        return;
    }

    let rows: Vec<Row> = app
        .backup_files
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Row::new(vec![Cell::from(name)])
        })
        .collect();

    let widths = [Constraint::Percentage(100)];
    let title = format!("Backups in {}", app.backup_folder.display());

    let table = Table::new(rows, widths)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");

    frame.render_stateful_widget(table, area, &mut app.backup_table_state);
}

fn draw_backup_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let name = app
        .backup_selected_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let message = format!(
        "Restore {name} over {}?\n\n\
         This overwrites your current keybindings file with the backup's contents.\n\
         Enter to confirm, Esc to cancel.",
        app.source_path.display()
    );

    let body = Paragraph::new(message)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL).title("Confirm restore"));
    frame.render_widget(body, area);
}

fn draw_duplicate_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let conflicting = app
        .shortcuts
        .iter()
        .find(|s| Some(s.line) == app.duplicate_conflict_line);
    let conflict_desc = conflicting
        .map(|s| format!("{} ({})", s.key_combo(), s.action()))
        .unwrap_or_else(|| "another shortcut".to_string());

    let message = match &app.duplicate_fix_display {
        Some(fixed) => format!(
            "{} is already used by {conflict_desc}.\n\n\
             Enter to use {fixed} instead and save.\n\
             Esc to cancel (keeps the original key).",
            app.duplicate_attempted_combo
        ),
        None => format!(
            "{} is already used by {conflict_desc}.\n\n\
             No unused modifier avoids this conflict.\n\
             Esc to cancel (keeps the original key).",
            app.duplicate_attempted_combo
        ),
    };

    let body = Paragraph::new(message)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL).title("Duplicate keybind"));
    frame.render_widget(body, area);
}

/// Build the footer's lines for the current mode. Takes the live terminal width because
/// `Mode::Normal`'s hint list is long enough to need wrapping (see `normal_mode_footer_lines`);
/// every other mode's footer is short and fixed regardless of width.
fn footer_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    match app.mode {
        Mode::EditKey => edit_footer_lines("key", app),
        Mode::EditTarget => edit_footer_lines("target", app),
        Mode::EditDescription => edit_footer_lines("description", app),
        Mode::EditMainMod => edit_footer_lines("$mainMod", app),
        Mode::AddShortcut => edit_footer_lines("new shortcut's description", app),
        Mode::SourcePath => edit_footer_lines("keybindings file", app),
        Mode::TemplateFolder => edit_footer_lines("template folder", app),
        Mode::TemplateSaveName => edit_footer_lines("template name", app),
        Mode::TemplateSaveSelect => vec![Line::from(
            "Space toggle   Enter continue   Esc cancel   ↑/k ↓/j move",
        )],
        Mode::TemplateList => vec![Line::from("Enter load   Esc cancel   ↑/k ↓/j move")],
        Mode::TemplatePreview => vec![Line::from(
            "Space toggle   Enter apply   Esc cancel   ↑/k ↓/j move",
        )],
        Mode::BackupFolder => edit_footer_lines("backup folder", app),
        Mode::TerminalCommand => edit_footer_lines("terminal command", app),
        Mode::BackupList => vec![Line::from("Enter select   Esc cancel   ↑/k ↓/j move")],
        Mode::BackupConfirm => vec![Line::from("Enter restore   Esc cancel")],
        Mode::DuplicateKeyConfirm => vec![Line::from(if app.duplicate_fix_display.is_some() {
            "Enter apply fix   Esc cancel"
        } else {
            "Esc cancel"
        })],
        Mode::Search => vec![Line::from(format!(
            "/{}▏   Enter apply   Esc cancel",
            app.query
        ))],
        Mode::Normal => {
            if let Some(error) = &app.error {
                vec![Line::from(error.clone()).style(Style::default().fg(Color::Red))]
            } else if let Some(status) = &app.status {
                vec![Line::from(status.clone()).style(Style::default().fg(Color::Green))]
            } else {
                normal_mode_footer_lines(app, width)
            }
        }
    }
}

/// Commands shown in the main (non-Settings) group of the Normal-mode footer, in display order.
/// The very first item changes depending on whether a search filter is active; see
/// `normal_mode_footer_lines`.
const NORMAL_MODE_COMMAND_ITEMS: [&str; 11] = [
    "e edit key",
    "a edit target",
    "d edit description",
    "A add shortcut",
    "o open terminal",
    "t save template",
    "l load template",
    "b backup",
    "r restore",
    "↑/k ↓/j move",
    "q quit",
];

/// Commands shown in the `Settings:` group of the Normal-mode footer: the file-path/folder/command
/// configuration commands, kept visually separate from the everyday browse/edit/backup actions
/// above so the latter don't get lost among them.
const SETTINGS_ITEMS: [&str; 5] = [
    "E edit $mainMod",
    "T template folder",
    "S keybindings file",
    "B backup folder",
    "O terminal command",
];

/// Build the Normal-mode footer's hint lines: a word-wrapped group of everyday commands, followed
/// by a word-wrapped, `Settings:`-labeled group of the configuration commands — both wrapped
/// independently to fit `width` so nothing is ever silently clipped, at any terminal width.
fn normal_mode_footer_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let width = width as usize;

    let mut commands: Vec<&str> = Vec::with_capacity(NORMAL_MODE_COMMAND_ITEMS.len() + 2);
    let filter_item;
    if app.query.is_empty() {
        commands.push("/ search");
    } else {
        filter_item = format!("filter: \"{}\"", app.query);
        commands.push(&filter_item);
        commands.push("/ change filter");
    }
    commands.extend(NORMAL_MODE_COMMAND_ITEMS);

    wrap_hints("", &commands, width)
        .into_iter()
        .chain(wrap_hints("Settings: ", &SETTINGS_ITEMS, width))
        .map(Line::from)
        .collect()
}

/// Greedily pack `items` (joined with three spaces, matching the footer's existing visual style)
/// onto as few lines as fit `max_width` columns each. `prefix` is prepended to the first line only
/// and counts toward its width; continuation lines start directly with the next item. An item
/// wider than `max_width` on its own still gets forced onto its own line rather than being split
/// or dropped, so nothing ever silently disappears. Width is measured in `char`s, matching how the
/// rest of the app measures text (e.g. `App::edit_cursor_byte_offset`), not display width.
fn wrap_hints(prefix: &str, items: &[&str], max_width: usize) -> Vec<String> {
    const SEP: &str = "   ";
    let sep_width = SEP.chars().count();

    let mut lines = Vec::new();
    let mut current = prefix.to_string();
    let mut current_width = prefix.chars().count();
    let mut current_has_items = false;

    for item in items {
        let item_width = item.chars().count();
        let needed = if current_has_items { sep_width + item_width } else { item_width };
        if current_has_items && current_width + needed > max_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
            current_has_items = false;
        }
        if current_has_items {
            current.push_str(SEP);
            current_width += sep_width;
        }
        current.push_str(item);
        current_width += item_width;
        current_has_items = true;
    }
    lines.push(current);
    lines
}

fn edit_footer_lines(field: &str, app: &App) -> Vec<Line<'static>> {
    let cursor_byte = app.edit_cursor_byte_offset();
    let before = app.edit_buffer[..cursor_byte].to_string();
    let mut rest = app.edit_buffer[cursor_byte..].chars();
    // Highlight the character the cursor sits on, like a terminal block cursor, instead of
    // inserting a separate cursor glyph that would shift everything after it over by a cell.
    let under_cursor = rest.next().unwrap_or(' ').to_string();
    let after: String = rest.collect();

    let prompt = Line::from(vec![
        Span::raw(format!("editing {field}: {before}")),
        Span::styled(under_cursor, Style::default().add_modifier(Modifier::REVERSED)),
        Span::raw(after),
    ]);

    vec![prompt, Line::from("Enter save   Esc cancel   ←/→ move   Home/End jump")]
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn wrap_hints_fits_everything_on_one_line_when_there_is_room() {
        let lines = wrap_hints("", &["a", "b", "c"], 80);
        assert_eq!(lines, vec!["a   b   c".to_string()]);
    }

    #[test]
    fn wrap_hints_wraps_onto_multiple_lines_and_none_exceed_max_width() {
        let items = ["e edit key", "a edit target", "d edit description", "q quit"];
        let lines = wrap_hints("", &items, 20);
        assert!(lines.len() > 1, "expected wrapping onto multiple lines, got {lines:?}");
        for line in &lines {
            assert!(line.chars().count() <= 20, "line exceeds max_width: {line:?}");
        }
        // Every item must still appear somewhere; wrapping must never drop content.
        for item in items {
            assert!(lines.iter().any(|line| line.contains(item)), "missing item: {item}");
        }
    }

    #[test]
    fn wrap_hints_forces_an_oversized_item_onto_its_own_line_rather_than_dropping_it() {
        let lines = wrap_hints("", &["short", "a very long item that alone exceeds the width"], 10);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "short");
        assert_eq!(lines[1], "a very long item that alone exceeds the width");
    }

    #[test]
    fn wrap_hints_counts_the_prefix_toward_the_first_lines_width() {
        // "Settings: " (10 chars) plus "one" (3) is 13, over a width of 12, so "one" should be
        // forced onto the first line anyway (prefix + one item always fits on the first line)
        // but a second item should overflow onto a new line.
        let lines = wrap_hints("Settings: ", &["one", "two"], 12);
        assert_eq!(lines, vec!["Settings: one".to_string(), "two".to_string()]);
    }

    #[test]
    fn wrap_hints_with_no_items_returns_just_the_prefix() {
        let lines = wrap_hints("Settings: ", &[], 80);
        assert_eq!(lines, vec!["Settings: ".to_string()]);
    }

    #[test]
    fn wrap_hints_with_no_items_and_no_prefix_returns_a_single_empty_line() {
        let lines: Vec<String> = wrap_hints("", &[], 80);
        assert_eq!(lines, vec![String::new()]);
    }
}
