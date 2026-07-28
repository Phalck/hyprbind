use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::widgets::TableState;

use crate::config::{self, Settings};
use crate::keybindings::{self, Shortcut, Variable};

mod backup;
mod edit;
mod lines;
mod navigation;
mod paths;
mod script;
mod search;
mod settings;
mod shortcuts;
mod template;
#[cfg(test)]
mod test_support;

use paths::{default_keybindings_path, home_dir};
use script::detect_terminal;

/// How long a status message stays in the footer before it's cleared automatically, so the
/// normal key-hint menu comes back without the user having to do anything.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    /// Editing the mods/key field of the selected shortcut.
    EditKey,
    /// Editing the dispatcher/args field of the selected shortcut.
    EditTarget,
    /// Editing the description of the selected shortcut.
    EditDescription,
    /// Editing the value of the `$mainMod` variable.
    EditMainMod,
    /// Entering the description for a new shortcut being created; see `App::start_add_shortcut`.
    AddShortcut,
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
    /// Editing the backup save/restore folder.
    BackupFolder,
    /// Editing the terminal command used by `o` (open a terminal at a shortcut's script).
    TerminalCommand,
    /// Picking which `.hbb` file to restore.
    BackupList,
    /// Confirming a restore before it overwrites the keybindings file.
    BackupConfirm,
    /// Confirming how to resolve a key combo colliding with another shortcut's, after editing
    /// a shortcut's key.
    DuplicateKeyConfirm,
    /// Confirming deletion of the selected shortcut before its line is removed from the file.
    DeleteConfirm,
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

    /// Folder backups are saved to and restored from. Defaults to `$HOME`.
    pub backup_folder: PathBuf,
    /// `.hbb` files found in `backup_folder`, shown by `Mode::BackupList`.
    pub backup_files: Vec<PathBuf>,
    pub backup_table_state: TableState,
    /// The backup picked in `Mode::BackupList`, awaiting confirmation in `Mode::BackupConfirm`.
    pub backup_selected_path: Option<PathBuf>,

    /// The program (and any fixed arguments) `o` uses to open a terminal, e.g. "kitty" or
    /// "alacritty --hold". `None` if nothing was persisted and auto-detection (`$TERMINAL`, then
    /// a `$PATH` scan for common terminals) came up empty too — `o` then tells the user to set
    /// one with `O` rather than trying to run a program that doesn't exist.
    pub terminal_command: Option<String>,

    /// The other shortcut a `Mode::EditKey` save collided with, pending confirmation in
    /// `Mode::DuplicateKeyConfirm`.
    pub duplicate_conflict_line: Option<usize>,
    /// Resolved display of the combo that was attempted, e.g. "SUPER + Q".
    pub duplicate_attempted_combo: String,
    /// Resolved display of the combo with the suggested fix applied, e.g. "SUPER + SHIFT + Q".
    /// `None` if no unused modifier resolves the conflict, in which case only cancelling is
    /// offered.
    pub duplicate_fix_display: Option<String>,
    /// Raw mods text to write (original raw mods plus the extra modifier) if the fix is
    /// accepted. `None` alongside `duplicate_fix_display: None`.
    duplicate_fix_mods_raw: Option<String>,
    /// Raw key text as typed, needed to rebuild the line if the fix is accepted.
    duplicate_key_raw: String,

    /// Where persisted settings are read from and written to. An explicit field (rather than
    /// always calling `config::config_path()` directly) so tests can point it at a scratch path
    /// instead of the real `~/.config/hyprbind/config`.
    config_path: PathBuf,
}

impl App {
    pub fn new() -> Self {
        let config_path = config::config_path();
        let settings = config::load_from(&config_path);
        let mut app = Self {
            source_path: settings
                .source_path
                .clone()
                .unwrap_or_else(default_keybindings_path),
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
            template_folder: settings.template_folder.clone().unwrap_or_else(home_dir),
            template_candidates: Vec::new(),
            template_selected: HashSet::new(),
            template_table_state: TableState::default(),
            template_files: Vec::new(),
            template_source_name: None,
            backup_folder: settings.backup_folder.clone().unwrap_or_else(home_dir),
            backup_files: Vec::new(),
            backup_table_state: TableState::default(),
            backup_selected_path: None,
            terminal_command: settings.terminal_command.clone().or_else(detect_terminal),
            duplicate_conflict_line: None,
            duplicate_attempted_combo: String::new(),
            duplicate_fix_display: None,
            duplicate_fix_mods_raw: None,
            duplicate_key_raw: String::new(),
            config_path,
        };
        app.load();

        // Whatever we started from (a persisted path or the hardcoded ML4W default) isn't there
        // or has nothing in it — fall back to searching the standard Hyprland config directory
        // for the real keybindings file, rather than just giving up. If that finds one, persist
        // it too: we only get here because the previous choice was already broken, so there's
        // nothing worth preserving by leaving it in place.
        if app.shortcuts.is_empty()
            && let Some(discovered) = keybindings::discover(&home_dir().join(".config/hypr"))
        {
            app.source_path = discovered.clone();
            app.load();
            if !app.shortcuts.is_empty() {
                let saved = app.persist_settings().is_ok();
                app.set_status(format!(
                    "Auto-detected keybindings file: {}{}",
                    discovered.display(),
                    if saved { " (saved)." } else { "." }
                ));
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
            self.shortcuts
                .iter()
                .filter(|s| s.matches(&query))
                .collect()
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

    /// Write the current `source_path`, `template_folder`, and `backup_folder` to
    /// `self.config_path`, so they're picked up again on the next launch instead of resetting to
    /// the hardcoded defaults. Best-effort: callers decide how (or whether) to surface a
    /// failure, since losing persistence is much less serious than losing the change it's
    /// persisting.
    fn persist_settings(&self) -> io::Result<()> {
        let settings = Settings {
            source_path: Some(self.source_path.clone()),
            template_folder: Some(self.template_folder.clone()),
            backup_folder: Some(self.backup_folder.clone()),
            terminal_command: self.terminal_command.clone(),
        };
        config::save_to(&self.config_path, &settings)
    }

    /// Clear `status` once it's been showing for `STATUS_TIMEOUT`, so the footer falls back to
    /// the normal key-hint menu on its own. Call this on every tick of the UI loop, not just on
    /// key presses, since the whole point is that it fires without user input.
    pub fn clear_expired_status(&mut self) {
        if self
            .status_set_at
            .is_some_and(|set_at| set_at.elapsed() >= STATUS_TIMEOUT)
        {
            self.clear_status();
        }
    }

    /// The raw source text last loaded at `line_no`, from whichever collection (shortcuts or
    /// variables) has it. Passed to `write_line` as an optimistic-concurrency check: if the file
    /// on disk no longer has this exact text at that line, it changed since the last load.
    fn expected_line_at(&self, line_no: usize) -> Option<&str> {
        self.shortcuts
            .iter()
            .find(|s| s.line == line_no)
            .map(|s| s.raw.as_str())
            .or_else(|| {
                self.variables
                    .iter()
                    .find(|v| v.line == line_no)
                    .map(|v| v.raw.as_str())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::edit_app;

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
}
