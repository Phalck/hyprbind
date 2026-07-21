use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::widgets::TableState;

use crate::keybindings::{self, Shortcut, Variable};

const TEMPLATE_EXTENSION: &str = "hbt";
/// How long a status message stays in the footer before it's cleared automatically, so the
/// normal key-hint menu comes back without the user having to do anything.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
}

/// Where the active Hyprland keybinding set lives, per the ML4W dotfiles layout.
fn default_keybindings_path() -> PathBuf {
    home_dir().join(".mydotfiles/com.ml4w.dotfiles/.config/hypr/conf/keybindings/default.conf")
}

/// Expand a leading `~` (a bare `~` or `~/...`) against `$HOME`. Any other input is used as-is.
fn expand_home(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        home_dir().join(rest)
    } else if input == "~" {
        home_dir()
    } else {
        PathBuf::from(input)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    /// Editing the mods/key field of the selected shortcut.
    EditKey,
    /// Editing the dispatcher/args field of the selected shortcut.
    EditTarget,
    /// Editing the value of the `$mainMod` variable.
    EditMainMod,
    /// Editing the keybindings file path.
    SourcePath,
    /// Editing the template save/load folder.
    TemplateFolder,
    /// Picking which visible shortcuts to save into a new template.
    TemplateSaveSelect,
    /// Naming the template file before it's written.
    TemplateSaveName,
    /// Picking which `.hbt` file to load.
    TemplateList,
    /// Picking which shortcuts from a loaded template to apply.
    TemplatePreview,
}

pub struct App {
    pub source_path: PathBuf,
    pub shortcuts: Vec<Shortcut>,
    pub variables: Vec<Variable>,
    pub table_state: TableState,
    /// Set when the keybindings file couldn't be read or parsed to nothing.
    pub error: Option<String>,
    pub query: String,
    pub mode: Mode,
    pub edit_buffer: String,
    /// Cursor position within `edit_buffer`, as a count of `char`s (not bytes) from the start.
    pub edit_cursor: usize,
    /// The source line number currently being edited, so a save knows where to splice.
    pub editing_line: Option<usize>,
    /// The shortcut selected when editing started, so a save can restore the selection even when
    /// the edited line isn't a shortcut at all (e.g. editing `$mainMod`).
    resume_line: Option<usize>,
    /// Transient message shown in the footer after a save (success or failure). Clears itself
    /// after `STATUS_TIMEOUT`; see `clear_expired_status`.
    pub status: Option<String>,
    status_set_at: Option<Instant>,

    /// Folder templates are saved to and loaded from. Defaults to `$HOME`.
    pub template_folder: PathBuf,
    /// The shortcuts being offered for pick: either the current view (when saving) or the
    /// contents of a loaded `.hbt` file (when applying). Shared between both flows since they're
    /// never active at the same time.
    pub template_candidates: Vec<Shortcut>,
    /// `.line` values (within whichever file `template_candidates` came from) that are checked.
    pub template_selected: HashSet<usize>,
    pub template_table_state: TableState,
    /// `.hbt` files found in `template_folder`, shown by `Mode::TemplateList`.
    pub template_files: Vec<PathBuf>,
    /// File name of the template currently being previewed, for display purposes.
    pub template_source_name: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self {
            source_path: default_keybindings_path(),
            shortcuts: Vec::new(),
            variables: Vec::new(),
            table_state: TableState::default(),
            error: None,
            query: String::new(),
            mode: Mode::Normal,
            edit_buffer: String::new(),
            edit_cursor: 0,
            editing_line: None,
            resume_line: None,
            status: None,
            status_set_at: None,
            template_folder: home_dir(),
            template_candidates: Vec::new(),
            template_selected: HashSet::new(),
            template_table_state: TableState::default(),
            template_files: Vec::new(),
            template_source_name: None,
        };
        app.load();

        // The hardcoded ML4W default isn't there (or has nothing in it) — fall back to
        // searching the standard Hyprland config directory for the real keybindings file,
        // rather than just giving up. Never overrides a path the user sets manually afterward.
        if app.shortcuts.is_empty() {
            if let Some(discovered) = keybindings::discover(&home_dir().join(".config/hypr")) {
                app.source_path = discovered.clone();
                app.load();
                if !app.shortcuts.is_empty() {
                    app.set_status(format!("Auto-detected keybindings file: {}", discovered.display()));
                }
            }
        }

        if !app.shortcuts.is_empty() {
            app.table_state.select_first();
        }
        app
    }

    fn load(&mut self) {
        match keybindings::parse_file(&self.source_path) {
            Ok(config) if config.shortcuts.is_empty() => {
                self.shortcuts = Vec::new();
                self.variables = config.variables;
                self.error = Some(format!(
                    "No shortcuts found in {}. Press S to set the keybindings file path.",
                    self.source_path.display()
                ));
            }
            Ok(config) => {
                self.shortcuts = config.shortcuts;
                self.variables = config.variables;
                self.error = None;
            }
            Err(err) => {
                self.shortcuts = Vec::new();
                self.variables = Vec::new();
                self.error = Some(format!(
                    "Couldn't read {}: {err}. Press S to set the keybindings file path.",
                    self.source_path.display()
                ));
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

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
        self.status_set_at = Some(Instant::now());
    }

    fn clear_status(&mut self) {
        self.status = None;
        self.status_set_at = None;
    }

    /// Clear `status` once it's been showing for `STATUS_TIMEOUT`, so the footer falls back to
    /// the normal key-hint menu on its own. Call this on every tick of the UI loop, not just on
    /// key presses, since the whole point is that it fires without user input.
    pub fn clear_expired_status(&mut self) {
        if self.status_set_at.is_some_and(|set_at| set_at.elapsed() >= STATUS_TIMEOUT) {
            self.clear_status();
        }
    }

    pub fn enter_search(&mut self) {
        self.clear_status();
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
        self.clear_status();
        self.edit_cursor = buffer.chars().count();
        self.edit_buffer = buffer;
        self.editing_line = Some(line);
        self.resume_line = Some(line);
        self.mode = mode;
    }

    /// Start editing the value of the `$mainMod` variable. Not tied to the current row selection
    /// (it's a config-wide setting, not a per-shortcut one), but the selected shortcut, if any,
    /// is remembered so it stays selected after the save reloads the list.
    pub fn start_edit_main_mod(&mut self) {
        let Some(var) = self.variables.iter().find(|v| v.name == "mainMod") else {
            self.set_status("No $mainMod variable found in the config.".to_string());
            return;
        };
        let (value, line) = (var.value.clone(), var.line);

        let current_idx = self.table_state.selected();
        let resume_line = current_idx.and_then(|idx| self.visible().get(idx).map(|s| s.line));

        self.clear_status();
        self.edit_cursor = value.chars().count();
        self.edit_buffer = value;
        self.editing_line = Some(line);
        self.resume_line = resume_line;
        self.mode = Mode::EditMainMod;
    }

    pub fn cancel_edit(&mut self) {
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.editing_line = None;
        self.resume_line = None;
        self.mode = Mode::Normal;
    }

    /// Insert `c` at the cursor and advance the cursor past it.
    pub fn push_edit_char(&mut self, c: char) {
        let byte_idx = self.edit_cursor_byte_offset();
        self.edit_buffer.insert(byte_idx, c);
        self.edit_cursor += 1;
    }

    /// Remove the character before the cursor (backspace).
    pub fn pop_edit_char(&mut self) {
        if self.edit_cursor == 0 {
            return;
        }
        let byte_idx = self.char_to_byte_offset(self.edit_cursor - 1);
        self.edit_buffer.remove(byte_idx);
        self.edit_cursor -= 1;
    }

    /// Remove the character at the cursor (forward delete).
    pub fn delete_edit_char(&mut self) {
        if self.edit_cursor >= self.edit_buffer.chars().count() {
            return;
        }
        let byte_idx = self.edit_cursor_byte_offset();
        self.edit_buffer.remove(byte_idx);
    }

    pub fn move_edit_cursor_left(&mut self) {
        self.edit_cursor = self.edit_cursor.saturating_sub(1);
    }

    pub fn move_edit_cursor_right(&mut self) {
        let len = self.edit_buffer.chars().count();
        if self.edit_cursor < len {
            self.edit_cursor += 1;
        }
    }

    pub fn move_edit_cursor_home(&mut self) {
        self.edit_cursor = 0;
    }

    pub fn move_edit_cursor_end(&mut self) {
        self.edit_cursor = self.edit_buffer.chars().count();
    }

    /// Byte offset in `edit_buffer` corresponding to `edit_cursor` (a char count), so callers can
    /// split or splice the buffer without risking a UTF-8 boundary panic.
    pub fn edit_cursor_byte_offset(&self) -> usize {
        self.char_to_byte_offset(self.edit_cursor)
    }

    fn char_to_byte_offset(&self, char_idx: usize) -> usize {
        self.edit_buffer
            .char_indices()
            .nth(char_idx)
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(self.edit_buffer.len())
    }

    /// Rebuild the source line from the edit buffer (interpreted according to which field is
    /// being edited) and write it back to `source_path` in place, then reload.
    pub fn save_edit(&mut self) {
        let Some(line_no) = self.editing_line else {
            return;
        };

        let new_line = match self.mode {
            Mode::EditKey => self.shortcuts.iter().find(|s| s.line == line_no).map(|shortcut| {
                let (mods_raw, key_raw) = match self.edit_buffer.split_once(',') {
                    Some((mods, key)) => (mods.trim(), key.trim()),
                    None => ("", self.edit_buffer.trim()),
                };
                shortcut.with_key(mods_raw, key_raw)
            }),
            Mode::EditTarget => self.shortcuts.iter().find(|s| s.line == line_no).map(|shortcut| {
                let (dispatcher_raw, args_raw) = match self.edit_buffer.split_once(',') {
                    Some((dispatcher, args)) => (dispatcher.trim(), args.trim()),
                    None => (self.edit_buffer.trim(), ""),
                };
                shortcut.with_target(dispatcher_raw, args_raw)
            }),
            Mode::EditMainMod => self.variables.iter().find(|v| v.line == line_no).map(|variable| {
                format!("${} = {}", variable.name, self.edit_buffer.trim())
            }),
            _ => None,
        };

        match new_line {
            Some(new_line) => match write_line(&self.source_path, line_no, &new_line) {
                Ok(()) => {
                    self.set_status("Saved.".to_string());
                    self.reload_and_reselect(self.resume_line.unwrap_or(line_no));
                }
                Err(err) => {
                    self.set_status(format!("Failed to save: {err}"));
                }
            },
            None => {
                self.set_status("Couldn't save: not found in the current file.".to_string());
            }
        }

        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.editing_line = None;
        self.resume_line = None;
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

    // ---- Keybindings file path ------------------------------------------------------------

    pub fn start_edit_source_path(&mut self) {
        self.clear_status();
        self.edit_buffer = self.source_path.display().to_string();
        self.edit_cursor = self.edit_buffer.chars().count();
        self.mode = Mode::SourcePath;
    }

    /// Switch to a different keybindings file, provided it actually parses and has at least one
    /// shortcut. Unlike the template folder, a bad value here is never silently accepted: this
    /// path controls what the app reads *and writes*, so `source_path` and the currently-loaded
    /// shortcuts are left untouched on any failure rather than leaving the user looking at a
    /// blank list with no explanation.
    pub fn save_source_path(&mut self) {
        let input = self.edit_buffer.trim();
        if input.is_empty() {
            self.set_status("Keybindings file path can't be empty.".to_string());
            return;
        }
        let expanded = expand_home(input);
        match keybindings::parse_file(&expanded) {
            Ok(config) if config.shortcuts.is_empty() => {
                self.set_status(format!(
                    "No shortcuts found in {}; keeping the current file.",
                    expanded.display()
                ));
            }
            Ok(config) => {
                self.source_path = expanded.clone();
                self.shortcuts = config.shortcuts;
                self.variables = config.variables;
                self.error = None;
                self.table_state.select_first();
                self.set_status(format!("Now using {}.", expanded.display()));
            }
            Err(err) => {
                self.set_status(format!("Couldn't read {}: {err}", expanded.display()));
            }
        }
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.mode = Mode::Normal;
    }

    // ---- Template folder ----------------------------------------------------------------

    pub fn start_edit_template_folder(&mut self) {
        self.clear_status();
        self.edit_buffer = self.template_folder.display().to_string();
        self.edit_cursor = self.edit_buffer.chars().count();
        self.mode = Mode::TemplateFolder;
    }

    pub fn save_template_folder(&mut self) {
        let input = self.edit_buffer.trim();
        if input.is_empty() {
            self.set_status("Template folder can't be empty.".to_string());
            return;
        }
        let expanded = expand_home(input);
        match fs::create_dir_all(&expanded) {
            Ok(()) => {
                self.set_status(format!("Template folder set to {}.", expanded.display()));
                self.template_folder = expanded;
            }
            Err(err) => {
                self.set_status(format!("Couldn't use {}: {err}", expanded.display()));
            }
        }
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.mode = Mode::Normal;
    }

    // ---- Save template --------------------------------------------------------------------

    /// Snapshot the currently visible shortcuts and open the save-template picker.
    pub fn start_template_save_select(&mut self) {
        self.clear_status();
        self.template_candidates = self.visible().into_iter().cloned().collect();
        self.template_selected.clear();
        self.template_table_state = TableState::default();
        if !self.template_candidates.is_empty() {
            self.template_table_state.select_first();
        }
        self.mode = Mode::TemplateSaveSelect;
    }

    pub fn template_select_next(&mut self) {
        if !self.template_candidates.is_empty() {
            self.template_table_state.select_next();
        }
    }

    pub fn template_select_previous(&mut self) {
        if !self.template_candidates.is_empty() {
            self.template_table_state.select_previous();
        }
    }

    pub fn toggle_template_selection(&mut self) {
        let Some(idx) = self.template_table_state.selected() else {
            return;
        };
        let Some(line) = self.template_candidates.get(idx).map(|s| s.line) else {
            return;
        };
        if !self.template_selected.insert(line) {
            self.template_selected.remove(&line);
        }
    }

    /// Move from picking rows to naming the file, if at least one row is checked.
    pub fn confirm_template_save_select(&mut self) {
        if self.template_selected.is_empty() {
            self.set_status("Select at least one shortcut first (Space to toggle).".to_string());
            return;
        }
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.mode = Mode::TemplateSaveName;
    }

    /// Write the checked shortcuts (resolved, no `$VAR` references) to
    /// `<template_folder>/<name>.hbt`.
    pub fn save_template(&mut self) {
        let name = self.edit_buffer.trim();
        if name.is_empty() {
            self.set_status("Template name can't be empty.".to_string());
            return;
        }
        if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
            self.set_status("Template name can't contain a path separator.".to_string());
            return;
        }

        let lines: Vec<String> = self
            .template_candidates
            .iter()
            .filter(|s| self.template_selected.contains(&s.line))
            .map(|s| s.resolved_line())
            .collect();
        let count = lines.len();

        let path = self.template_folder.join(format!("{name}.{TEMPLATE_EXTENSION}"));
        match write_template(&self.template_folder, &path, &lines) {
            Ok(()) => {
                self.set_status(format!("Saved {count} shortcut(s) to {}.", path.display()));
            }
            Err(err) => {
                self.set_status(format!("Failed to save template: {err}"));
            }
        }

        self.cancel_template();
    }

    // ---- Load template --------------------------------------------------------------------

    pub fn start_template_list(&mut self) {
        self.clear_status();
        self.template_files = list_template_files(&self.template_folder);
        self.template_table_state = TableState::default();
        if !self.template_files.is_empty() {
            self.template_table_state.select_first();
        }
        self.mode = Mode::TemplateList;
    }

    pub fn template_list_select_next(&mut self) {
        if !self.template_files.is_empty() {
            self.template_table_state.select_next();
        }
    }

    pub fn template_list_select_previous(&mut self) {
        if !self.template_files.is_empty() {
            self.template_table_state.select_previous();
        }
    }

    /// Parse the selected `.hbt` file and open the apply picker, with every shortcut in it
    /// checked by default.
    pub fn open_selected_template(&mut self) {
        let Some(idx) = self.template_table_state.selected() else {
            return;
        };
        let Some(path) = self.template_files.get(idx).cloned() else {
            return;
        };

        match keybindings::parse_file(&path) {
            Ok(config) if config.shortcuts.is_empty() => {
                self.set_status(format!("No shortcuts found in {}.", path.display()));
            }
            Ok(config) => {
                self.template_selected = config.shortcuts.iter().map(|s| s.line).collect();
                self.template_candidates = config.shortcuts;
                self.template_table_state = TableState::default();
                self.template_table_state.select_first();
                self.template_source_name =
                    path.file_name().map(|n| n.to_string_lossy().into_owned());
                self.mode = Mode::TemplatePreview;
            }
            Err(err) => {
                self.set_status(format!("Couldn't read {}: {err}", path.display()));
            }
        }
    }

    /// Append the checked shortcuts to `source_path`, skipping any that would collide with an
    /// existing shortcut's key combo.
    pub fn apply_template_selection(&mut self) {
        if self.template_selected.is_empty() {
            self.set_status("Select at least one shortcut to apply (Space to toggle).".to_string());
            return;
        }

        let mut lines = Vec::new();
        let mut skipped = 0;
        for candidate in &self.template_candidates {
            if !self.template_selected.contains(&candidate.line) {
                continue;
            }
            if self.shortcuts.iter().any(|existing| existing.same_combo(candidate)) {
                skipped += 1;
            } else {
                lines.push(candidate.resolved_line());
            }
        }

        if lines.is_empty() {
            self.set_status(format!("Nothing applied: {skipped} shortcut(s) already bound."));
            self.cancel_template();
            return;
        }

        let applied = lines.len();
        let resume_line = self
            .table_state
            .selected()
            .and_then(|idx| self.visible().get(idx).map(|s| s.line));

        match append_lines(&self.source_path, self.template_source_name.as_deref(), &lines) {
            Ok(()) => {
                self.set_status(if skipped > 0 {
                    format!("Applied {applied}, skipped {skipped} (already bound).")
                } else {
                    format!("Applied {applied} shortcut(s).")
                });
                self.cancel_template();
                match resume_line {
                    Some(line) => self.reload_and_reselect(line),
                    None => {
                        self.load();
                        self.table_state.select_first();
                    }
                }
            }
            Err(err) => {
                self.set_status(format!("Failed to apply: {err}"));
            }
        }
    }

    /// Abandon whichever template flow is active (save or load) and return to normal browsing.
    pub fn cancel_template(&mut self) {
        self.template_candidates.clear();
        self.template_selected.clear();
        self.template_table_state = TableState::default();
        self.template_files.clear();
        self.template_source_name = None;
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.mode = Mode::Normal;
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

/// Atomically write `contents` to `path` (write to a sibling `.tmp` file, then rename over the
/// original) so a failed write can never leave a partially-written file behind.
fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let tmp_path = {
        let mut s = path.as_os_str().to_owned();
        s.push(".tmp");
        PathBuf::from(s)
    };
    fs::write(&tmp_path, contents)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Read `path`, replace line `line_no` with `new_line`, and write the result back atomically.
fn write_line(path: &Path, line_no: usize, new_line: &str) -> io::Result<()> {
    let contents = fs::read_to_string(path)?;
    let updated = replace_line(&contents, line_no, new_line).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "source file changed on disk; reload and try again",
        )
    })?;
    write_atomic(path, &updated)
}

/// Append `new_lines` to the end of `path`, preceded by a blank line and a marker comment, and
/// write the result back atomically. Every existing line is left untouched.
fn append_lines(path: &Path, source_label: Option<&str>, new_lines: &[String]) -> io::Result<()> {
    let mut contents = fs::read_to_string(path)?;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push('\n');
    match source_label {
        Some(label) => contents.push_str(&format!("# Applied from template: {label}\n")),
        None => contents.push_str("# Applied from template\n"),
    }
    for line in new_lines {
        contents.push_str(line);
        contents.push('\n');
    }
    write_atomic(path, &contents)
}

/// Write `lines` (already newline-terminated content, one shortcut per line) to `path`,
/// creating `folder` first if it doesn't exist yet.
fn write_template(folder: &Path, path: &Path, lines: &[String]) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let mut contents = String::new();
    for line in lines {
        contents.push_str(line);
        contents.push('\n');
    }
    write_atomic(path, &contents)
}

/// List `.hbt` files directly inside `folder`, sorted by name. Returns an empty list if the
/// folder doesn't exist or can't be read, rather than failing.
fn list_template_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(TEMPLATE_EXTENSION))
        .collect();
    files.sort();
    files
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

    #[test]
    fn expand_home_handles_tilde_forms() {
        let home = home_dir();
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/Templates"), home.join("Templates"));
        assert_eq!(expand_home("/etc/foo"), PathBuf::from("/etc/foo"));
    }

    fn edit_app() -> App {
        App {
            source_path: PathBuf::from("/dev/null"),
            shortcuts: Vec::new(),
            variables: Vec::new(),
            table_state: TableState::default(),
            error: None,
            query: String::new(),
            mode: Mode::EditKey,
            edit_buffer: String::new(),
            edit_cursor: 0,
            editing_line: None,
            resume_line: None,
            status: None,
            status_set_at: None,
            template_folder: PathBuf::from("/dev/null"),
            template_candidates: Vec::new(),
            template_selected: HashSet::new(),
            template_table_state: TableState::default(),
            template_files: Vec::new(),
            template_source_name: None,
        }
    }

    #[test]
    fn push_edit_char_inserts_at_cursor_not_only_at_end() {
        let mut app = edit_app();
        app.edit_buffer = "ac".to_string();
        app.edit_cursor = 1;
        app.push_edit_char('b');
        assert_eq!(app.edit_buffer, "abc");
        assert_eq!(app.edit_cursor, 2);
    }

    #[test]
    fn pop_edit_char_removes_before_cursor() {
        let mut app = edit_app();
        app.edit_buffer = "abc".to_string();
        app.edit_cursor = 2;
        app.pop_edit_char();
        assert_eq!(app.edit_buffer, "ac");
        assert_eq!(app.edit_cursor, 1);
    }

    #[test]
    fn pop_edit_char_at_start_of_buffer_does_nothing() {
        let mut app = edit_app();
        app.edit_buffer = "abc".to_string();
        app.edit_cursor = 0;
        app.pop_edit_char();
        assert_eq!(app.edit_buffer, "abc");
        assert_eq!(app.edit_cursor, 0);
    }

    #[test]
    fn delete_edit_char_removes_character_at_cursor() {
        let mut app = edit_app();
        app.edit_buffer = "abc".to_string();
        app.edit_cursor = 1;
        app.delete_edit_char();
        assert_eq!(app.edit_buffer, "ac");
        assert_eq!(app.edit_cursor, 1);
    }

    #[test]
    fn cursor_movement_is_clamped_to_buffer_bounds() {
        let mut app = edit_app();
        app.edit_buffer = "ab".to_string();

        app.move_edit_cursor_left();
        assert_eq!(app.edit_cursor, 0, "can't move left of the start");

        app.move_edit_cursor_end();
        assert_eq!(app.edit_cursor, 2);
        app.move_edit_cursor_right();
        assert_eq!(app.edit_cursor, 2, "can't move right of the end");

        app.move_edit_cursor_home();
        assert_eq!(app.edit_cursor, 0);
    }

    #[test]
    fn cursor_operations_respect_utf8_character_boundaries() {
        let mut app = edit_app();
        app.edit_buffer = "aé—b".to_string();
        app.edit_cursor = 4;
        app.pop_edit_char();
        assert_eq!(app.edit_buffer, "aé—");
        assert_eq!(app.edit_cursor, 3);
    }

    #[test]
    fn toggle_template_selection_toggles_by_line_number() {
        let mut app = edit_app();
        app.template_candidates = vec![sample_shortcut(1, "Q"), sample_shortcut(2, "W")];
        app.template_table_state.select(Some(1));

        app.toggle_template_selection();
        assert!(app.template_selected.contains(&2));

        app.toggle_template_selection();
        assert!(!app.template_selected.contains(&2));
    }

    #[test]
    fn confirm_template_save_select_requires_a_selection() {
        let mut app = edit_app();
        app.mode = Mode::TemplateSaveSelect;
        app.confirm_template_save_select();
        assert_eq!(app.mode, Mode::TemplateSaveSelect);
        assert!(app.status.is_some());
    }

    #[test]
    fn confirm_template_save_select_moves_to_naming_when_something_is_checked() {
        let mut app = edit_app();
        app.mode = Mode::TemplateSaveSelect;
        app.template_selected.insert(1);
        app.confirm_template_save_select();
        assert_eq!(app.mode, Mode::TemplateSaveName);
    }

    #[test]
    fn set_status_records_when_it_was_set() {
        let mut app = edit_app();
        app.set_status("hi");
        assert_eq!(app.status.as_deref(), Some("hi"));
        assert!(app.status_set_at.is_some());
    }

    #[test]
    fn clear_expired_status_clears_once_the_timeout_has_passed() {
        let mut app = edit_app();
        app.status = Some("old".to_string());
        app.status_set_at = Some(Instant::now() - STATUS_TIMEOUT - Duration::from_millis(1));
        app.clear_expired_status();
        assert!(app.status.is_none());
    }

    #[test]
    fn clear_expired_status_leaves_a_fresh_status_alone() {
        let mut app = edit_app();
        app.set_status("fresh");
        app.clear_expired_status();
        assert_eq!(app.status.as_deref(), Some("fresh"));
    }

    #[test]
    fn save_source_path_rejects_empty_input() {
        let mut app = edit_app();
        let original = app.source_path.clone();
        app.edit_buffer = "   ".to_string();
        app.save_source_path();
        assert_eq!(app.source_path, original);
        assert!(app.status.is_some());
    }

    #[test]
    fn save_source_path_rejects_a_file_with_zero_shortcuts_and_keeps_the_old_one() {
        let path = std::env::temp_dir()
            .join(format!("hyprbind-test-source-empty-{}.conf", std::process::id()));
        fs::write(&path, "# just a comment\n").unwrap();

        let mut app = edit_app();
        let original_path = app.source_path.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.edit_buffer = path.display().to_string();
        app.save_source_path();

        assert_eq!(app.source_path, original_path);
        assert_eq!(app.shortcuts.len(), 1, "old shortcuts must survive a rejected path");
        assert!(app.status.is_some());

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn save_source_path_rejects_a_missing_file_and_keeps_the_old_one() {
        let missing = std::env::temp_dir().join("hyprbind-test-source-missing-hopefully.conf");
        let mut app = edit_app();
        let original_path = app.source_path.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.edit_buffer = missing.display().to_string();
        app.save_source_path();

        assert_eq!(app.source_path, original_path);
        assert_eq!(app.shortcuts.len(), 1);
    }

    #[test]
    fn save_source_path_switches_to_a_valid_file() {
        let path = std::env::temp_dir()
            .join(format!("hyprbind-test-source-valid-{}.conf", std::process::id()));
        fs::write(&path, "bind = SUPER, Q, killactive\n").unwrap();

        let mut app = edit_app();
        app.edit_buffer = path.display().to_string();
        app.save_source_path();

        assert_eq!(app.source_path, path);
        assert_eq!(app.shortcuts.len(), 1);
        assert_eq!(app.mode, Mode::Normal);

        fs::remove_file(&path).unwrap();
    }

    fn sample_shortcut(line: usize, key: &str) -> Shortcut {
        Shortcut {
            bind_type: "bind".to_string(),
            mods: vec!["SUPER".to_string()],
            key: key.to_string(),
            description: None,
            dispatcher: "exec".to_string(),
            args: "foo".to_string(),
            comment: None,
            line,
            raw: format!("bind = $mainMod, {key}, exec, foo"),
            mods_raw: "$mainMod".to_string(),
            key_raw: key.to_string(),
            description_raw: None,
            dispatcher_raw: "exec".to_string(),
            args_raw: "foo".to_string(),
        }
    }

    #[test]
    fn write_template_creates_folder_and_writes_resolved_lines() {
        let dir = std::env::temp_dir().join(format!("hyprbind-test-{}", std::process::id()));
        let folder = dir.join("templates");
        let path = folder.join("test.hbt");

        let lines = vec![sample_shortcut(1, "Q").resolved_line()];
        write_template(&folder, &path, &lines).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "bind = SUPER, Q, exec, foo\n");
        assert!(!fs::exists(path.with_extension("hbt.tmp")).unwrap_or(false));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn append_lines_adds_marker_comment_and_preserves_existing_content() {
        let path = std::env::temp_dir().join(format!("hyprbind-test-append-{}.conf", std::process::id()));
        fs::write(&path, "bind = $mainMod, Q, killactive\n").unwrap();

        append_lines(&path, Some("gaming.hbt"), &["bind = SUPER, W, exec, foo".to_string()]).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            "bind = $mainMod, Q, killactive\n\n# Applied from template: gaming.hbt\nbind = SUPER, W, exec, foo\n"
        );

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn list_template_files_filters_by_extension_and_sorts() {
        let dir = std::env::temp_dir().join(format!("hyprbind-test-list-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("b.hbt"), "").unwrap();
        fs::write(dir.join("a.hbt"), "").unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();

        let files = list_template_files(&dir);
        assert_eq!(files, vec![dir.join("a.hbt"), dir.join("b.hbt")]);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_template_files_on_missing_folder_returns_empty() {
        let missing = std::env::temp_dir().join("hyprbind-does-not-exist-hopefully");
        assert!(list_template_files(&missing).is_empty());
    }
}
