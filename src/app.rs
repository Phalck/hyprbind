use std::path::PathBuf;

use ratatui::widgets::TableState;

use crate::keybindings::{self, Shortcut};

/// Where the active Hyprland keybinding set lives, per the ML4W dotfiles layout.
fn default_keybindings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(
        ".mydotfiles/com.ml4w.dotfiles/.config/hypr/conf/keybindings/default.conf",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
}

pub struct App {
    pub source_path: PathBuf,
    pub shortcuts: Vec<Shortcut>,
    pub table_state: TableState,
    /// Set when the keybindings file couldn't be read or parsed to nothing.
    pub error: Option<String>,
    pub query: String,
    pub mode: Mode,
}

impl App {
    pub fn new() -> Self {
        let source_path = default_keybindings_path();
        let (shortcuts, error) = match keybindings::parse_file(&source_path) {
            Ok(shortcuts) if shortcuts.is_empty() => (
                Vec::new(),
                Some(format!("No shortcuts found in {}", source_path.display())),
            ),
            Ok(shortcuts) => (shortcuts, None),
            Err(err) => (
                Vec::new(),
                Some(format!("Couldn't read {}: {err}", source_path.display())),
            ),
        };

        let mut table_state = TableState::default();
        if !shortcuts.is_empty() {
            table_state.select_first();
        }

        Self {
            source_path,
            shortcuts,
            table_state,
            error,
            query: String::new(),
            mode: Mode::Normal,
        }
    }

    /// Shortcuts matching the current search query, in source order.
    pub fn visible(&self) -> Vec<&Shortcut> {
        if self.query.is_empty() {
            self.shortcuts.iter().collect()
        } else {
            let query = self.query.to_lowercase();
            self.shortcuts.iter().filter(|s| s.matches(&query)).collect()
        }
    }

    pub fn enter_search(&mut self) {
        self.mode = Mode::Search;
    }

    pub fn confirm_search(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn cancel_search(&mut self) {
        self.query.clear();
        self.mode = Mode::Normal;
        self.table_state.select_first();
    }

    pub fn push_query_char(&mut self, c: char) {
        self.query.push(c);
        self.table_state.select_first();
    }

    pub fn pop_query_char(&mut self) {
        self.query.pop();
        self.table_state.select_first();
    }

    pub fn select_next(&mut self) {
        if !self.visible().is_empty() {
            self.table_state.select_next();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.visible().is_empty() {
            self.table_state.select_previous();
        }
    }

    pub fn select_first(&mut self) {
        if !self.visible().is_empty() {
            self.table_state.select_first();
        }
    }

    pub fn select_last(&mut self) {
        if !self.visible().is_empty() {
            self.table_state.select_last();
        }
    }
}
