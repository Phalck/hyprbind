use std::io;
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
    /// Editing the mods/key field of the selected shortcut.
    EditKey,
    /// Editing the dispatcher/args field of the selected shortcut.
    EditTarget,
}

pub struct App {
    pub source_path: PathBuf,
    pub shortcuts: Vec<Shortcut>,
    pub table_state: TableState,
    /// Set when the keybindings file couldn't be read or parsed to nothing.
    pub error: Option<String>,
    pub query: String,
    pub mode: Mode,
    pub edit_buffer: String,
    /// The source line number currently being edited, so a save knows where to splice.
    pub editing_line: Option<usize>,
    /// Transient message shown in the footer after a save (success or failure).
    pub status: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let source_path = default_keybindings_path();
        let mut app = Self {
            source_path,
            shortcuts: Vec::new(),
            table_state: TableState::default(),
            error: None,
            query: String::new(),
            mode: Mode::Normal,
            edit_buffer: String::new(),
            editing_line: None,
            status: None,
        };
        app.load();
        if !app.shortcuts.is_empty() {
            app.table_state.select_first();
        }
        app
    }

    fn load(&mut self) {
        match keybindings::parse_file(&self.source_path) {
            Ok(shortcuts) if shortcuts.is_empty() => {
                self.shortcuts = Vec::new();
                self.error = Some(format!("No shortcuts found in {}", self.source_path.display()));
            }
            Ok(shortcuts) => {
                self.shortcuts = shortcuts;
                self.error = None;
            }
            Err(err) => {
                self.shortcuts = Vec::new();
                self.error = Some(format!("Couldn't read {}: {err}", self.source_path.display()));
            }
        }
    }

    /// Reload from disk and try to keep the selection on the shortcut that used to be at
    /// `target_line`, falling back to the first visible row if it's gone (e.g. the edit turned
    /// it into something the parser no longer recognizes as a bind).
    fn reload_and_reselect(&mut self, target_line: usize) {
        self.load();
        let pos = self.visible().iter().position(|s| s.line == target_line);
        let visible_len = self.visible().len();
        match pos {
            Some(pos) => self.table_state.select(Some(pos)),
            None if visible_len == 0 => self.table_state.select(None),
            None => self.table_state.select_first(),
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
        self.status = None;
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

    /// Start editing the mods/key field of the currently selected shortcut.
    pub fn start_edit_key(&mut self) {
        self.start_edit(Mode::EditKey, |s| s.key_edit_buffer());
    }

    /// Start editing the dispatcher/args field of the currently selected shortcut.
    pub fn start_edit_target(&mut self) {
        self.start_edit(Mode::EditTarget, |s| s.target_edit_buffer());
    }

    fn start_edit(&mut self, mode: Mode, buffer_for: impl FnOnce(&Shortcut) -> String) {
        let Some(idx) = self.table_state.selected() else {
            return;
        };
        let selected = self.visible().get(idx).map(|s| (buffer_for(s), s.line));
        let Some((buffer, line)) = selected else {
            return;
        };
        self.status = None;
        self.edit_buffer = buffer;
        self.editing_line = Some(line);
        self.mode = mode;
    }

    pub fn cancel_edit(&mut self) {
        self.edit_buffer.clear();
        self.editing_line = None;
        self.mode = Mode::Normal;
    }

    pub fn push_edit_char(&mut self, c: char) {
        self.edit_buffer.push(c);
    }

    pub fn pop_edit_char(&mut self) {
        self.edit_buffer.pop();
    }

    /// Rebuild the source line from the edit buffer (interpreted according to which field is
    /// being edited) and write it back to `source_path` in place, then reload.
    pub fn save_edit(&mut self) {
        let Some(line_no) = self.editing_line else {
            return;
        };
        let shortcut = self.shortcuts.iter().find(|s| s.line == line_no);
        let new_line = match (self.mode, shortcut) {
            (Mode::EditKey, Some(shortcut)) => {
                let (mods_raw, key_raw) = match self.edit_buffer.split_once(',') {
                    Some((mods, key)) => (mods.trim(), key.trim()),
                    None => ("", self.edit_buffer.trim()),
                };
                Some(shortcut.with_key(mods_raw, key_raw))
            }
            (Mode::EditTarget, Some(shortcut)) => {
                let (dispatcher_raw, args_raw) = match self.edit_buffer.split_once(',') {
                    Some((dispatcher, args)) => (dispatcher.trim(), args.trim()),
                    None => (self.edit_buffer.trim(), ""),
                };
                Some(shortcut.with_target(dispatcher_raw, args_raw))
            }
            _ => None,
        };

        match new_line {
            Some(new_line) => match write_line(&self.source_path, line_no, &new_line) {
                Ok(()) => {
                    self.status = Some("Saved.".to_string());
                    self.reload_and_reselect(line_no);
                }
                Err(err) => {
                    self.status = Some(format!("Failed to save: {err}"));
                }
            },
            None => {
                self.status = Some("Couldn't save: shortcut no longer found.".to_string());
            }
        }

        self.edit_buffer.clear();
        self.editing_line = None;
        self.mode = Mode::Normal;
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

/// Replace line `line_no` (1-based) of `contents` with `new_line`, preserving every other line
/// and whether the file ended with a trailing newline. Returns `None` if `line_no` is out of
/// range for `contents` (e.g. the file changed on disk since it was parsed).
fn replace_line(contents: &str, line_no: usize, new_line: &str) -> Option<String> {
    let mut lines: Vec<&str> = contents.lines().collect();
    let idx = line_no.checked_sub(1)?;
    if idx >= lines.len() {
        return None;
    }
    lines[idx] = new_line;

    let mut result = lines.join("\n");
    if contents.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

/// Read `path`, replace line `line_no` with `new_line`, and write the result back atomically
/// (write to a sibling temp file, then rename over the original) so a failed write can never
/// leave a partially-written config behind for Hyprland to pick up.
fn write_line(path: &std::path::Path, line_no: usize, new_line: &str) -> io::Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let updated = replace_line(&contents, line_no, new_line).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "source file changed on disk; reload and try again",
        )
    })?;

    let tmp_path = {
        let mut s = path.as_os_str().to_owned();
        s.push(".tmp");
        PathBuf::from(s)
    };
    std::fs::write(&tmp_path, &updated)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_line_swaps_only_target_line() {
        let contents = "one\ntwo\nthree\n";
        let updated = replace_line(contents, 2, "TWO").unwrap();
        assert_eq!(updated, "one\nTWO\nthree\n");
    }

    #[test]
    fn replace_line_preserves_missing_trailing_newline() {
        let contents = "one\ntwo\nthree";
        let updated = replace_line(contents, 1, "ONE").unwrap();
        assert_eq!(updated, "ONE\ntwo\nthree");
    }

    #[test]
    fn replace_line_out_of_range_returns_none() {
        let contents = "one\ntwo\n";
        assert!(replace_line(contents, 5, "x").is_none());
        assert!(replace_line(contents, 0, "x").is_none());
    }
}
