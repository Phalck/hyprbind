mod app;
mod keybindings;
mod ui;

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use app::{App, Mode};

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

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
                    KeyCode::Char('t') => app.start_edit_target(),
                    _ => {}
                },
                Mode::Search => match key.code {
                    KeyCode::Enter => app.confirm_search(),
                    KeyCode::Esc => app.cancel_search(),
                    KeyCode::Backspace => app.pop_query_char(),
                    KeyCode::Down => app.select_next(),
                    KeyCode::Up => app.select_previous(),
                    KeyCode::Char(c)
                        if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_query_char(c);
                    }
                    _ => {}
                },
                Mode::EditKey | Mode::EditTarget => match key.code {
                    KeyCode::Enter => app.save_edit(),
                    KeyCode::Esc => app.cancel_edit(),
                    KeyCode::Backspace => app.pop_edit_char(),
                    KeyCode::Char(c)
                        if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        app.push_edit_char(c);
                    }
                    _ => {}
                },
            }
        }
    }

    Ok(())
}
