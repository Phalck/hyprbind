mod app;
mod config;
mod fs_util;
mod keybindings;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use app::{App, Mode};

/// How often the loop wakes up even without input, so a status message set by
/// `App::set_status` can expire and clear itself on its own.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// `env!` values baked in by `build.rs`/Cargo at compile time, not runtime config.
const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("HYPRBIND_GIT_HASH");

fn is_version_flag(arg: &str) -> bool {
    arg == "--version" || arg == "-V"
}

fn main() -> io::Result<()> {
    if std::env::args()
        .nth(1)
        .is_some_and(|arg| is_version_flag(&arg))
    {
        println!("hyprbind {VERSION} ({GIT_HASH})");
        return Ok(());
    }

    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        app.clear_expired_status();
        terminal.draw(|frame| ui::draw(frame, app))?;

        if !event::poll(POLL_INTERVAL)? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            match app.mode {
                Mode::Normal => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Down | KeyCode::Char('j') => app.select_next(),
                    KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
                    KeyCode::Char('g') => app.select_first(),
                    KeyCode::Char('G') => app.select_last(),
                    KeyCode::Char('/') => app.enter_search(),
                    KeyCode::Char('e') => app.start_edit_key(),
                    KeyCode::Char('a') => app.start_edit_target(),
                    KeyCode::Char('d') => app.start_edit_description(),
                    KeyCode::Char('A') => app.start_add_shortcut(),
                    KeyCode::Char('x') => app.start_delete_shortcut(),
                    KeyCode::Char('E') => app.start_edit_main_mod(),
                    KeyCode::Char('t') => app.start_template_save_select(),
                    KeyCode::Char('l') => app.start_template_list(),
                    KeyCode::Char('T') => app.start_edit_template_folder(),
                    KeyCode::Char('S') => app.start_edit_source_path(),
                    KeyCode::Char('b') => app.create_backup(),
                    KeyCode::Char('r') => app.start_backup_list(),
                    KeyCode::Char('B') => app.start_edit_backup_folder(),
                    KeyCode::Char('o') => app.open_terminal_at_script(),
                    KeyCode::Char('O') => app.start_edit_terminal_command(),
                    _ => {}
                },
                Mode::Search => match key.code {
                    KeyCode::Enter => app.confirm_search(),
                    KeyCode::Esc => app.cancel_search(),
                    KeyCode::Backspace => app.pop_query_char(),
                    KeyCode::Down => app.select_next(),
                    KeyCode::Up => app.select_previous(),
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_query_char(c);
                    }
                    _ => {}
                },
                Mode::EditKey | Mode::EditTarget | Mode::EditDescription | Mode::EditMainMod => {
                    match key.code {
                        KeyCode::Enter => app.save_edit(),
                        KeyCode::Esc => app.cancel_edit(),
                        KeyCode::Backspace => app.pop_edit_char(),
                        KeyCode::Delete => app.delete_edit_char(),
                        KeyCode::Left => app.move_edit_cursor_left(),
                        KeyCode::Right => app.move_edit_cursor_right(),
                        KeyCode::Home => app.move_edit_cursor_home(),
                        KeyCode::End => app.move_edit_cursor_end(),
                        KeyCode::Char(c)
                            if !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                        {
                            app.push_edit_char(c);
                        }
                        _ => {}
                    }
                }
                Mode::AddShortcut => match key.code {
                    KeyCode::Enter => app.save_new_shortcut(),
                    KeyCode::Esc => app.cancel_edit(),
                    KeyCode::Backspace => app.pop_edit_char(),
                    KeyCode::Delete => app.delete_edit_char(),
                    KeyCode::Left => app.move_edit_cursor_left(),
                    KeyCode::Right => app.move_edit_cursor_right(),
                    KeyCode::Home => app.move_edit_cursor_home(),
                    KeyCode::End => app.move_edit_cursor_end(),
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_edit_char(c);
                    }
                    _ => {}
                },
                Mode::SourcePath => match key.code {
                    KeyCode::Enter => app.save_source_path(),
                    KeyCode::Esc => app.cancel_edit(),
                    KeyCode::Backspace => app.pop_edit_char(),
                    KeyCode::Delete => app.delete_edit_char(),
                    KeyCode::Left => app.move_edit_cursor_left(),
                    KeyCode::Right => app.move_edit_cursor_right(),
                    KeyCode::Home => app.move_edit_cursor_home(),
                    KeyCode::End => app.move_edit_cursor_end(),
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_edit_char(c);
                    }
                    _ => {}
                },
                Mode::TemplateFolder => match key.code {
                    KeyCode::Enter => app.save_template_folder(),
                    KeyCode::Esc => app.cancel_edit(),
                    KeyCode::Backspace => app.pop_edit_char(),
                    KeyCode::Delete => app.delete_edit_char(),
                    KeyCode::Left => app.move_edit_cursor_left(),
                    KeyCode::Right => app.move_edit_cursor_right(),
                    KeyCode::Home => app.move_edit_cursor_home(),
                    KeyCode::End => app.move_edit_cursor_end(),
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_edit_char(c);
                    }
                    _ => {}
                },
                Mode::TemplateSaveName => match key.code {
                    KeyCode::Enter => app.save_template(),
                    KeyCode::Esc => app.cancel_template(),
                    KeyCode::Backspace => app.pop_edit_char(),
                    KeyCode::Delete => app.delete_edit_char(),
                    KeyCode::Left => app.move_edit_cursor_left(),
                    KeyCode::Right => app.move_edit_cursor_right(),
                    KeyCode::Home => app.move_edit_cursor_home(),
                    KeyCode::End => app.move_edit_cursor_end(),
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_edit_char(c);
                    }
                    _ => {}
                },
                Mode::TemplateSaveSelect => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.template_select_next(),
                    KeyCode::Up | KeyCode::Char('k') => app.template_select_previous(),
                    KeyCode::Char(' ') => app.toggle_template_selection(),
                    KeyCode::Enter => app.confirm_template_save_select(),
                    KeyCode::Esc => app.cancel_template(),
                    _ => {}
                },
                Mode::TemplatePreview => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.template_select_next(),
                    KeyCode::Up | KeyCode::Char('k') => app.template_select_previous(),
                    KeyCode::Char(' ') => app.toggle_template_selection(),
                    KeyCode::Enter => app.apply_template_selection(),
                    KeyCode::Esc => app.cancel_template(),
                    _ => {}
                },
                Mode::TemplateList => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.template_list_select_next(),
                    KeyCode::Up | KeyCode::Char('k') => app.template_list_select_previous(),
                    KeyCode::Enter => app.open_selected_template(),
                    KeyCode::Esc => app.cancel_template(),
                    _ => {}
                },
                Mode::TerminalCommand => match key.code {
                    KeyCode::Enter => app.save_terminal_command(),
                    KeyCode::Esc => app.cancel_edit(),
                    KeyCode::Backspace => app.pop_edit_char(),
                    KeyCode::Delete => app.delete_edit_char(),
                    KeyCode::Left => app.move_edit_cursor_left(),
                    KeyCode::Right => app.move_edit_cursor_right(),
                    KeyCode::Home => app.move_edit_cursor_home(),
                    KeyCode::End => app.move_edit_cursor_end(),
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_edit_char(c);
                    }
                    _ => {}
                },
                Mode::BackupFolder => match key.code {
                    KeyCode::Enter => app.save_backup_folder(),
                    KeyCode::Esc => app.cancel_edit(),
                    KeyCode::Backspace => app.pop_edit_char(),
                    KeyCode::Delete => app.delete_edit_char(),
                    KeyCode::Left => app.move_edit_cursor_left(),
                    KeyCode::Right => app.move_edit_cursor_right(),
                    KeyCode::Home => app.move_edit_cursor_home(),
                    KeyCode::End => app.move_edit_cursor_end(),
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_edit_char(c);
                    }
                    _ => {}
                },
                Mode::BackupList => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.backup_list_select_next(),
                    KeyCode::Up | KeyCode::Char('k') => app.backup_list_select_previous(),
                    KeyCode::Enter => app.confirm_backup_selection(),
                    KeyCode::Esc => app.cancel_backup_restore(),
                    _ => {}
                },
                Mode::BackupConfirm => match key.code {
                    KeyCode::Enter => app.restore_backup(),
                    KeyCode::Esc => app.cancel_backup_restore(),
                    _ => {}
                },
                Mode::DuplicateKeyConfirm => match key.code {
                    KeyCode::Enter => app.accept_duplicate_fix(),
                    KeyCode::Esc => app.cancel_duplicate_confirm(),
                    _ => {}
                },
                Mode::DeleteConfirm => match key.code {
                    KeyCode::Enter => app.confirm_delete_shortcut(),
                    KeyCode::Esc => app.cancel_delete_shortcut(),
                    _ => {}
                },
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_version_flag_recognizes_long_and_short_forms() {
        assert!(is_version_flag("--version"));
        assert!(is_version_flag("-V"));
    }

    #[test]
    fn is_version_flag_rejects_anything_else() {
        assert!(!is_version_flag("-v"));
        assert!(!is_version_flag("--help"));
        assert!(!is_version_flag(""));
    }
}
