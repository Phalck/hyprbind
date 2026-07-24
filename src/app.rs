use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use ratatui::widgets::TableState;

use crate::config::{self, Settings};
use crate::fs_util::write_atomic;
use crate::keybindings::{self, Shortcut, SourceFormat, Variable};

const TEMPLATE_EXTENSION: &str = "hbt";
const BACKUP_EXTENSION: &str = "hbb";
/// Modifiers tried, in order, as a candidate fix when an edited key combo collides with another
/// shortcut's. Standard Hyprland modifier names, not anything ML4W- or config-specific.
const STANDARD_MODIFIERS: [&str; 4] = ["SUPER", "SHIFT", "CTRL", "ALT"];
/// How long a status message stays in the footer before it's cleared automatically, so the
/// normal key-hint menu comes back without the user having to do anything.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
/// Placeholder key a freshly-added shortcut (see `App::save_new_shortcut`) is bound to until `e`
/// replaces it: not a real Hyprland keysym, so Hyprland's own reload never actually binds
/// anything to it in the meantime.
const NEW_SHORTCUT_KEY: &str = "CHANGEME";
/// Placeholder dispatcher a freshly-added shortcut is bound to until `a` replaces it.
const NEW_SHORTCUT_DISPATCHER: &str = "exec";

/// Path for a new backup file, disambiguated if `{stem}-{timestamp}.{BACKUP_EXTENSION}` already
/// exists (e.g. two backups triggered within the same second, since `timestamp_string` only has
/// 1-second resolution). Appends `-2`, `-3`, ... before the extension until a free name is found,
/// so a second `b` press never silently overwrites the first backup.
fn unique_backup_path(dir: &Path, stem: &str, timestamp: &str) -> PathBuf {
    let base = dir.join(format!("{stem}-{timestamp}.{BACKUP_EXTENSION}"));
    if !base.exists() {
        return base;
    }
    (2..)
        .map(|n| dir.join(format!("{stem}-{timestamp}-{n}.{BACKUP_EXTENSION}")))
        .find(|candidate| !candidate.exists())
        .expect("infinite iterator always yields a free path")
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
}

/// A local-time, filename-safe timestamp like `20260721-153045`, via the system `date` command
/// (no calendar-math or timezone handling of our own, and no dependency: `date` is standard on
/// every Linux system this app targets). Falls back to raw Unix-epoch seconds if `date` can't be
/// run for some reason, which is still unique and sortable, just less human-readable.
fn timestamp_string() -> String {
    std::process::Command::new("date")
        .arg("+%Y%m%d-%H%M%S")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string())
        })
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

/// Common terminal emulators to look for on `$PATH`, in preference order, when nothing more
/// specific (`$TERMINAL`, a persisted setting) says which one to use.
const CANDIDATE_TERMINALS: [&str; 6] =
    ["kitty", "alacritty", "wezterm", "foot", "konsole", "xterm"];

/// Pick a default terminal command: `$TERMINAL` if it's set to something non-empty, otherwise
/// the first of `CANDIDATE_TERMINALS` found as a file in some `$PATH` entry. `None` if neither
/// turns up anything, in which case `o` tells the user to set one with `O` instead of guessing.
fn detect_terminal() -> Option<String> {
    if let Ok(term) = std::env::var("TERMINAL")
        && !term.trim().is_empty()
    {
        return Some(term);
    }

    let path_var = std::env::var_os("PATH")?;
    CANDIDATE_TERMINALS
        .into_iter()
        .find(|candidate| std::env::split_paths(&path_var).any(|dir| dir.join(candidate).is_file()))
        .map(String::from)
}

/// If `command` looks like it runs a script — any whitespace-separated token, `~`-expanded,
/// resolves to an existing file on disk — the parent directory of the first such token. Handles
/// both a script run directly (`~/foo.sh`) and one run through an interpreter (`bash ~/foo.sh`,
/// where the first token isn't a path at all). Returns `None` for a command with no file token,
/// e.g. a pipeline of system commands like `hyprctl activewindow | grep pid | xargs kill`.
fn script_directory(command: &str) -> Option<PathBuf> {
    command
        .split_whitespace()
        .map(expand_home)
        .find(|path| path.is_file())
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Launch `command` (a program name plus any fixed arguments, e.g. "kitty --hold") with its
/// working directory set to `dir`, detached from hyprbind's own stdio so it can't write into the
/// running TUI. Fire-and-forget: never waited on, so the event loop isn't blocked while the
/// terminal stays open.
fn spawn_terminal(command: &str, dir: &Path) -> io::Result<()> {
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty terminal command"))?;
    std::process::Command::new(program)
        .args(parts)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

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
}

/// The result of `App::check_key_conflict`: which other shortcut a candidate combo collides
/// with, and — if one exists — an unused modifier that would resolve it.
struct KeyConflict {
    conflicting_line: usize,
    attempted_display: String,
    fix: Option<KeyConflictFix>,
}

struct KeyConflictFix {
    mods_raw: String,
    display_combo: String,
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

    /// Start editing the description of the currently selected shortcut.
    pub fn start_edit_description(&mut self) {
        self.start_edit(Mode::EditDescription, |s| s.description_edit_buffer());
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
    /// being edited) and write it back to `source_path` in place, then reload. For `EditKey`,
    /// first checks whether the new combo would collide with another shortcut's; if so, this
    /// defers to `Mode::DuplicateKeyConfirm` instead of writing anything.
    pub fn save_edit(&mut self) {
        let Some(line_no) = self.editing_line else {
            return;
        };

        let new_line = match self.mode {
            Mode::EditKey => {
                let (mods_raw, key_raw) = match self.edit_buffer.split_once(',') {
                    Some((mods, key)) => (mods.trim().to_string(), key.trim().to_string()),
                    None => (String::new(), self.edit_buffer.trim().to_string()),
                };

                if let Some(conflict) = self.check_key_conflict(line_no, &mods_raw, &key_raw) {
                    self.duplicate_conflict_line = Some(conflict.conflicting_line);
                    self.duplicate_attempted_combo = conflict.attempted_display;
                    self.duplicate_fix_display =
                        conflict.fix.as_ref().map(|f| f.display_combo.clone());
                    self.duplicate_fix_mods_raw = conflict.fix.map(|f| f.mods_raw);
                    self.duplicate_key_raw = key_raw;
                    self.mode = Mode::DuplicateKeyConfirm;
                    return;
                }

                self.shortcuts
                    .iter()
                    .find(|s| s.line == line_no)
                    .map(|shortcut| shortcut.with_key(&mods_raw, &key_raw))
            }
            Mode::EditTarget => self
                .shortcuts
                .iter()
                .find(|s| s.line == line_no)
                .map(|shortcut| {
                    let (dispatcher_raw, args_raw) = match self.edit_buffer.split_once(',') {
                        Some((dispatcher, args)) => (dispatcher.trim(), args.trim()),
                        None => (self.edit_buffer.trim(), ""),
                    };
                    shortcut.with_target(dispatcher_raw, args_raw)
                }),
            Mode::EditDescription => {
                let description = self.edit_buffer.trim();
                if description.is_empty() {
                    self.set_status("Description can't be empty.".to_string());
                    return;
                }
                self.shortcuts
                    .iter()
                    .find(|s| s.line == line_no)
                    .map(|shortcut| shortcut.with_description(description))
            }
            Mode::EditMainMod => {
                self.variables
                    .iter()
                    .find(|v| v.line == line_no)
                    .map(|variable| {
                        let value = self.edit_buffer.trim();
                        let mut line = match variable.format {
                            SourceFormat::Conf => format!("${} = {value}", variable.name),
                            SourceFormat::Lua => format!("local {} = \"{value}\"", variable.name),
                        };
                        if let Some(comment) = &variable.comment {
                            line.push_str(" -- ");
                            line.push_str(comment);
                        }
                        line
                    })
            }
            _ => None,
        };

        self.commit_edit(new_line);
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

    /// Write `new_line` to `source_path` at `editing_line` (if present), report the result, and
    /// return to `Mode::Normal`. Shared by the direct `save_edit` path and, after a detour
    /// through `Mode::DuplicateKeyConfirm`, `accept_duplicate_fix`.
    fn commit_edit(&mut self, new_line: Option<String>) {
        let Some(line_no) = self.editing_line else {
            return;
        };

        match new_line {
            Some(new_line) => {
                let expected_old_line = self.expected_line_at(line_no).map(str::to_string);
                match write_line(
                    &self.source_path,
                    line_no,
                    expected_old_line.as_deref(),
                    &new_line,
                ) {
                    Ok(()) => {
                        self.set_status("Saved.".to_string());
                        self.reload_and_reselect(self.resume_line.unwrap_or(line_no));
                    }
                    Err(err) => {
                        self.set_status(format!("Failed to save: {err}"));
                    }
                }
            }
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

    /// Whether `mods_raw`/`key_raw` (as typed into the "edit key" field) would resolve to a
    /// combo already used by some *other* shortcut (`editing_line` is excluded, so re-saving a
    /// shortcut's own unchanged key is never flagged against itself).
    fn check_key_conflict(
        &self,
        editing_line: usize,
        mods_raw: &str,
        key_raw: &str,
    ) -> Option<KeyConflict> {
        let mods = self.resolve_mods(mods_raw);
        let key = self.resolve_vars(key_raw);

        let conflicting_line = self
            .shortcuts
            .iter()
            .find(|s| s.line != editing_line && s.matches_combo(&mods, &key))?
            .line;

        let attempted_display = if mods.is_empty() {
            key.clone()
        } else {
            format!("{} + {key}", mods.join(" + "))
        };

        // Try each standard modifier not already present; the first one that both doesn't
        // collide with anything else and isn't already part of the attempted combo wins.
        let fix = STANDARD_MODIFIERS
            .iter()
            .filter(|candidate| !mods.iter().any(|m| m == *candidate))
            .find_map(|candidate| {
                let mut trial_mods = mods.clone();
                trial_mods.push((*candidate).to_string());
                let collides = self
                    .shortcuts
                    .iter()
                    .any(|s| s.line != editing_line && s.matches_combo(&trial_mods, &key));
                if collides {
                    return None;
                }
                let fixed_mods_raw = if mods_raw.trim().is_empty() {
                    candidate.to_string()
                } else {
                    format!("{} {candidate}", mods_raw.trim())
                };
                Some(KeyConflictFix {
                    mods_raw: fixed_mods_raw,
                    display_combo: format!("{} + {key}", trial_mods.join(" + ")),
                })
            });

        Some(KeyConflict {
            conflicting_line,
            attempted_display,
            fix,
        })
    }

    /// Resolve `$VAR` references in `raw` against the currently-loaded variables, mirroring how
    /// the parser resolves them when loading the file. Used to compare a not-yet-written combo
    /// against already-loaded (and thus already-resolved) shortcuts on equal terms.
    fn resolve_vars(&self, raw: &str) -> String {
        let mut result = raw.to_string();
        for var in &self.variables {
            result = result.replace(&format!("${}", var.name), &var.value);
        }
        result
    }

    fn resolve_mods(&self, mods_raw: &str) -> Vec<String> {
        self.resolve_vars(mods_raw)
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }

    /// Accept the suggested fix (if any) from `Mode::DuplicateKeyConfirm`: add the extra
    /// modifier and save. If there's nothing to accept (no unused modifier resolved the
    /// conflict), this just cancels instead.
    pub fn accept_duplicate_fix(&mut self) {
        let (Some(mods_raw), Some(line_no)) =
            (self.duplicate_fix_mods_raw.clone(), self.editing_line)
        else {
            self.cancel_duplicate_confirm();
            return;
        };
        let key_raw = self.duplicate_key_raw.clone();
        let new_line = self
            .shortcuts
            .iter()
            .find(|s| s.line == line_no)
            .map(|shortcut| shortcut.with_key(&mods_raw, &key_raw));

        self.clear_duplicate_state();
        self.commit_edit(new_line);
    }

    /// Abandon the duplicate-key flow and the edit that triggered it; nothing is written.
    pub fn cancel_duplicate_confirm(&mut self) {
        self.clear_duplicate_state();
        self.cancel_edit();
    }

    fn clear_duplicate_state(&mut self) {
        self.duplicate_conflict_line = None;
        self.duplicate_attempted_combo.clear();
        self.duplicate_fix_display = None;
        self.duplicate_fix_mods_raw = None;
        self.duplicate_key_raw.clear();
    }

    // ---- Add shortcut ----------------------------------------------------------------------

    /// Start adding a new shortcut: prompts for its description first (`Mode::AddShortcut`);
    /// `save_new_shortcut` appends it once that's entered, with placeholder key/target fields
    /// left for `e`/`a` to fill in afterward. Conf-only, like `apply_template_selection`: there's
    /// no reliable way to guess a valid Lua `hl.bind` dispatcher call to seed a brand new binding
    /// with.
    pub fn start_add_shortcut(&mut self) {
        if self.source_format() == SourceFormat::Lua {
            self.set_status(
                "Adding shortcuts isn't supported yet for Lua-format keybinding files.".to_string(),
            );
            return;
        }
        self.clear_status();
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.mode = Mode::AddShortcut;
    }

    /// Append a new shortcut, using the description just entered in `Mode::AddShortcut` and
    /// placeholder key/dispatcher fields (`NEW_SHORTCUT_KEY`/`NEW_SHORTCUT_DISPATCHER`), then
    /// select it so `e`/`a` are ready to replace those placeholders.
    pub fn save_new_shortcut(&mut self) {
        let description = self.edit_buffer.trim();
        if description.is_empty() {
            self.set_status("Description can't be empty.".to_string());
            return;
        }

        let line =
            format!("bind = , {NEW_SHORTCUT_KEY}, {NEW_SHORTCUT_DISPATCHER} # {description}");
        match append_lines(&self.source_path, "# Added via hyprbind", &[line]) {
            Ok(()) => {
                self.load();
                match self.shortcuts.iter().map(|s| s.line).max() {
                    Some(line_no) => {
                        let pos = self.visible().iter().position(|s| s.line == line_no);
                        self.table_state.select(pos);
                        self.set_status(format!(
                            "Added. Press e to set its key (currently {NEW_SHORTCUT_KEY}) and a to set its target."
                        ));
                    }
                    None => {
                        self.set_status("Added, but couldn't find it after reloading.".to_string());
                    }
                }
            }
            Err(err) => self.set_status(format!("Failed to add: {err}")),
        }

        self.edit_buffer.clear();
        self.edit_cursor = 0;
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
                let saved = self.persist_settings().is_ok();
                self.set_status(format!(
                    "Now using {}{}",
                    expanded.display(),
                    if saved { " (saved)." } else { "." }
                ));
            }
            Err(err) => {
                self.set_status(format!("Couldn't read {}: {err}", expanded.display()));
            }
        }
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.mode = Mode::Normal;
    }

    /// Which keybinding syntax `source_path` is in. Template save/apply is `.conf`-syntax only
    /// (see `start_template_save_select` and `apply_template_selection`), since there's no
    /// reliable way to translate an arbitrary Lua `hl.dsp....` dispatcher call into a Hyprland
    /// `.conf` dispatcher, or vice versa.
    fn source_format(&self) -> SourceFormat {
        keybindings::format_for_path(&self.source_path)
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
                self.template_folder = expanded.clone();
                let saved = self.persist_settings().is_ok();
                self.set_status(format!(
                    "Template folder set to {}{}",
                    expanded.display(),
                    if saved { " (saved)." } else { "." }
                ));
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
        if self.source_format() == SourceFormat::Lua {
            self.set_status(
                "Saving templates isn't supported yet for Lua-format keybinding files.".to_string(),
            );
            return;
        }
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

        let path = self
            .template_folder
            .join(format!("{name}.{TEMPLATE_EXTENSION}"));
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
        self.template_files = list_files_with_extension(&self.template_folder, TEMPLATE_EXTENSION);
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
        if self.source_format() == SourceFormat::Lua {
            self.set_status(
                "Applying templates isn't supported yet for Lua-format keybinding files."
                    .to_string(),
            );
            self.cancel_template();
            return;
        }
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
            if self
                .shortcuts
                .iter()
                .any(|existing| existing.same_combo(candidate))
            {
                skipped += 1;
            } else {
                lines.push(candidate.resolved_line());
            }
        }

        if lines.is_empty() {
            self.set_status(format!(
                "Nothing applied: {skipped} shortcut(s) already bound."
            ));
            self.cancel_template();
            return;
        }

        let applied = lines.len();
        let resume_line = self
            .table_state
            .selected()
            .and_then(|idx| self.visible().get(idx).map(|s| s.line));

        let marker = match self.template_source_name.as_deref() {
            Some(label) => format!("# Applied from template: {label}"),
            None => "# Applied from template".to_string(),
        };
        match append_lines(&self.source_path, &marker, &lines) {
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

    // ---- Backup folder ----------------------------------------------------------------

    pub fn start_edit_backup_folder(&mut self) {
        self.clear_status();
        self.edit_buffer = self.backup_folder.display().to_string();
        self.edit_cursor = self.edit_buffer.chars().count();
        self.mode = Mode::BackupFolder;
    }

    pub fn save_backup_folder(&mut self) {
        let input = self.edit_buffer.trim();
        if input.is_empty() {
            self.set_status("Backup folder can't be empty.".to_string());
            return;
        }
        let expanded = expand_home(input);
        match fs::create_dir_all(&expanded) {
            Ok(()) => {
                self.backup_folder = expanded.clone();
                let saved = self.persist_settings().is_ok();
                self.set_status(format!(
                    "Backup folder set to {}{}",
                    expanded.display(),
                    if saved { " (saved)." } else { "." }
                ));
            }
            Err(err) => {
                self.set_status(format!("Couldn't use {}: {err}", expanded.display()));
            }
        }
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.mode = Mode::Normal;
    }

    // ---- Backup -------------------------------------------------------------------------

    /// Copy the current keybindings file, byte-for-byte, into a new timestamped file in the
    /// backup folder. A plain copy rather than anything routed through the parser: a backup
    /// exists to put the file back exactly as it was, so it needs to preserve everything —
    /// comments, `$VAR` definitions, exact formatting — not just the shortcuts we understand.
    pub fn create_backup(&mut self) {
        if let Err(err) = fs::create_dir_all(&self.backup_folder) {
            self.set_status(format!(
                "Couldn't use {}: {err}",
                self.backup_folder.display()
            ));
            return;
        }

        let stem = self
            .source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("hyprbind");
        let path = unique_backup_path(&self.backup_folder, stem, &timestamp_string());

        match fs::copy(&self.source_path, &path) {
            Ok(_) => self.set_status(format!("Backed up to {}.", path.display())),
            Err(err) => {
                self.set_status(format!(
                    "Backup failed: couldn't read {}: {err}",
                    self.source_path.display()
                ));
            }
        }
    }

    // ---- Restore from backup ------------------------------------------------------------

    pub fn start_backup_list(&mut self) {
        self.clear_status();
        self.backup_files = list_files_with_extension(&self.backup_folder, BACKUP_EXTENSION);
        self.backup_table_state = TableState::default();
        if !self.backup_files.is_empty() {
            self.backup_table_state.select_first();
        }
        self.mode = Mode::BackupList;
    }

    pub fn backup_list_select_next(&mut self) {
        if !self.backup_files.is_empty() {
            self.backup_table_state.select_next();
        }
    }

    pub fn backup_list_select_previous(&mut self) {
        if !self.backup_files.is_empty() {
            self.backup_table_state.select_previous();
        }
    }

    /// Move from the backup list to the restore confirmation step, without touching anything
    /// yet — restoring overwrites the whole keybindings file, so it's the one destructive action
    /// in the app that gets a dedicated "are you sure" step rather than committing immediately.
    pub fn confirm_backup_selection(&mut self) {
        let Some(idx) = self.backup_table_state.selected() else {
            return;
        };
        let Some(path) = self.backup_files.get(idx).cloned() else {
            return;
        };
        self.backup_selected_path = Some(path);
        self.mode = Mode::BackupConfirm;
    }

    /// Abandon the restore flow (from either the list or the confirmation step) and return to
    /// normal browsing without changing anything.
    pub fn cancel_backup_restore(&mut self) {
        self.backup_files.clear();
        self.backup_table_state = TableState::default();
        self.backup_selected_path = None;
        self.mode = Mode::Normal;
    }

    /// Overwrite `source_path` with the selected backup's exact contents.
    pub fn restore_backup(&mut self) {
        let Some(path) = self.backup_selected_path.clone() else {
            self.mode = Mode::Normal;
            return;
        };

        match fs::read_to_string(&path) {
            Ok(contents) => match write_atomic(&self.source_path, &contents) {
                Ok(()) => {
                    self.set_status(format!(
                        "Restored {} from {}.",
                        self.source_path.display(),
                        path.display()
                    ));
                    self.load();
                    if self.shortcuts.is_empty() {
                        self.table_state.select(None);
                    } else {
                        self.table_state.select_first();
                    }
                }
                Err(err) => {
                    self.set_status(format!("Restore failed: {err}"));
                }
            },
            Err(err) => {
                self.set_status(format!("Couldn't read {}: {err}", path.display()));
            }
        }

        self.backup_files.clear();
        self.backup_table_state = TableState::default();
        self.backup_selected_path = None;
        self.mode = Mode::Normal;
    }

    // ---- Terminal command ----------------------------------------------------------------

    pub fn start_edit_terminal_command(&mut self) {
        self.clear_status();
        self.edit_buffer = self.terminal_command.clone().unwrap_or_default();
        self.edit_cursor = self.edit_buffer.chars().count();
        self.mode = Mode::TerminalCommand;
    }

    /// Unlike the keybindings file path, a bad value here is never rejected outright: there's no
    /// cheap, reliable way to validate an arbitrary command line, so whatever is typed is
    /// accepted and only found out to be wrong (if it is) when `o` tries to spawn it.
    pub fn save_terminal_command(&mut self) {
        let input = self.edit_buffer.trim();
        if input.is_empty() {
            self.set_status("Terminal command can't be empty.".to_string());
            return;
        }
        self.terminal_command = Some(input.to_string());
        let saved = self.persist_settings().is_ok();
        self.set_status(format!(
            "Terminal command set to {input}{}",
            if saved { " (saved)." } else { "." }
        ));
        self.edit_buffer.clear();
        self.edit_cursor = 0;
        self.mode = Mode::Normal;
    }

    // ---- Open a terminal at a shortcut's script -------------------------------------------

    /// Open a terminal in the directory containing the script the selected shortcut's action
    /// runs, if it has one. Reports a specific status message at whichever step comes up empty,
    /// rather than doing nothing silently: the action isn't `exec`, the command doesn't point at
    /// a real script, or no terminal command is available to run.
    pub fn open_terminal_at_script(&mut self) {
        let Some(idx) = self.table_state.selected() else {
            return;
        };
        let Some(shortcut) = self.visible().get(idx).map(|s| (*s).clone()) else {
            return;
        };

        let Some(command) = shortcut.exec_command() else {
            self.set_status("Not a script: this shortcut doesn't run exec.".to_string());
            return;
        };
        let Some(dir) = script_directory(&command) else {
            self.set_status(
                "Not a script: couldn't find a script file in this command.".to_string(),
            );
            return;
        };
        let Some(terminal_command) = self.terminal_command.clone() else {
            self.set_status("No terminal command set. Press O to set one.".to_string());
            return;
        };

        match spawn_terminal(&terminal_command, &dir) {
            Ok(()) => self.set_status(format!("Opened a terminal at {}.", dir.display())),
            Err(err) => self.set_status(format!("Couldn't open a terminal: {err}")),
        }
    }
}

/// Replace line `line_no` (1-based) of `contents` with `new_line`, preserving every other line
/// and whether the file ended with a trailing newline. Returns `None` if `line_no` is out of
/// range for `contents`, or if `expected_old_line` is given and doesn't match what's actually
/// there — either way, the file changed on disk since it was parsed.
fn replace_line(
    contents: &str,
    line_no: usize,
    expected_old_line: Option<&str>,
    new_line: &str,
) -> Option<String> {
    let mut lines: Vec<&str> = contents.lines().collect();
    let idx = line_no.checked_sub(1)?;
    if idx >= lines.len() {
        return None;
    }
    if let Some(expected) = expected_old_line
        && lines[idx] != expected
    {
        return None;
    }
    lines[idx] = new_line;

    let mut result = lines.join("\n");
    if contents.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

/// Read `path`, replace line `line_no` with `new_line`, and write the result back atomically.
/// `expected_old_line`, when given, must match the line's current on-disk text or the write is
/// refused — guards against clobbering a change made by another hyprbind instance or a hand edit
/// since this line was last loaded (the file can change without its line count changing, which a
/// bounds check alone wouldn't catch).
fn write_line(
    path: &Path,
    line_no: usize,
    expected_old_line: Option<&str>,
    new_line: &str,
) -> io::Result<()> {
    let contents = fs::read_to_string(path)?;
    let updated =
        replace_line(&contents, line_no, expected_old_line, new_line).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "source file changed on disk; reload and try again",
            )
        })?;
    write_atomic(path, &updated)
}

/// Append `new_lines` to the end of `path`, preceded by a blank line and `marker` (a full `#
/// ...` comment line), and write the result back atomically. Every existing line is left
/// untouched.
fn append_lines(path: &Path, marker: &str, new_lines: &[String]) -> io::Result<()> {
    let mut contents = fs::read_to_string(path)?;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push('\n');
    contents.push_str(marker);
    contents.push('\n');
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

/// List files directly inside `folder` whose extension matches `extension`, sorted by name.
/// Returns an empty list if the folder doesn't exist or can't be read, rather than failing.
fn list_files_with_extension(folder: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(extension))
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
        let updated = replace_line(contents, 2, None, "TWO").unwrap();
        assert_eq!(updated, "one\nTWO\nthree\n");
    }

    #[test]
    fn replace_line_preserves_missing_trailing_newline() {
        let contents = "one\ntwo\nthree";
        let updated = replace_line(contents, 1, None, "ONE").unwrap();
        assert_eq!(updated, "ONE\ntwo\nthree");
    }

    #[test]
    fn replace_line_out_of_range_returns_none() {
        let contents = "one\ntwo\n";
        assert!(replace_line(contents, 5, None, "x").is_none());
        assert!(replace_line(contents, 0, None, "x").is_none());
    }

    #[test]
    fn replace_line_matching_expected_old_line_succeeds() {
        let contents = "one\ntwo\nthree\n";
        let updated = replace_line(contents, 2, Some("two"), "TWO").unwrap();
        assert_eq!(updated, "one\nTWO\nthree\n");
    }

    #[test]
    fn replace_line_refuses_when_expected_old_line_does_not_match() {
        let contents = "one\ntwo\nthree\n";
        assert!(
            replace_line(contents, 2, Some("something else"), "TWO").is_none(),
            "the file changed since expected_old_line was captured, so the write must be refused"
        );
    }

    #[test]
    fn expand_home_handles_tilde_forms() {
        let home = home_dir();
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/Templates"), home.join("Templates"));
        assert_eq!(expand_home("/etc/foo"), PathBuf::from("/etc/foo"));
    }

    #[test]
    fn script_directory_resolves_a_direct_script_invocation() {
        let script = std::env::temp_dir().join(format!(
            "hyprbind-test-script-direct-{}.sh",
            std::process::id()
        ));
        fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();

        assert_eq!(
            script_directory(script.to_str().unwrap()),
            script.parent().map(Path::to_path_buf)
        );

        fs::remove_file(&script).unwrap();
    }

    #[test]
    fn script_directory_resolves_a_script_run_through_an_interpreter() {
        let script = std::env::temp_dir().join(format!(
            "hyprbind-test-script-interp-{}.sh",
            std::process::id()
        ));
        fs::write(&script, "echo hi\n").unwrap();
        let command = format!("bash {}", script.display());

        assert_eq!(
            script_directory(&command),
            script.parent().map(Path::to_path_buf)
        );

        fs::remove_file(&script).unwrap();
    }

    #[test]
    fn script_directory_is_none_without_a_file_token() {
        assert_eq!(
            script_directory("hyprctl activewindow | grep pid | xargs kill"),
            None
        );
        assert_eq!(
            script_directory("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+"),
            None
        );
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
            backup_folder: PathBuf::from("/dev/null"),
            backup_files: Vec::new(),
            backup_table_state: TableState::default(),
            backup_selected_path: None,
            terminal_command: None,
            duplicate_conflict_line: None,
            duplicate_attempted_combo: String::new(),
            duplicate_fix_display: None,
            duplicate_fix_mods_raw: None,
            duplicate_key_raw: String::new(),
            // Guaranteed to fail (/dev/null isn't a directory, so create_dir_all on any path
            // under it errors out) so a stray persist_settings() call in a test can never write
            // to a real location, let alone the user's actual ~/.config/hyprbind/config.
            config_path: PathBuf::from("/dev/null/hyprbind-test-config"),
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
        let path = std::env::temp_dir().join(format!(
            "hyprbind-test-source-empty-{}.conf",
            std::process::id()
        ));
        fs::write(&path, "# just a comment\n").unwrap();

        let mut app = edit_app();
        let original_path = app.source_path.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.edit_buffer = path.display().to_string();
        app.save_source_path();

        assert_eq!(app.source_path, original_path);
        assert_eq!(
            app.shortcuts.len(),
            1,
            "old shortcuts must survive a rejected path"
        );
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
        let path = std::env::temp_dir().join(format!(
            "hyprbind-test-source-valid-{}.conf",
            std::process::id()
        ));
        fs::write(&path, "bind = SUPER, Q, killactive\n").unwrap();

        let mut app = edit_app();
        app.edit_buffer = path.display().to_string();
        app.save_source_path();

        assert_eq!(app.source_path, path);
        assert_eq!(app.shortcuts.len(), 1);
        assert_eq!(app.mode, Mode::Normal);

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn save_source_path_persists_to_config_path() {
        let target = std::env::temp_dir().join(format!(
            "hyprbind-test-source-persist-target-{}.conf",
            std::process::id()
        ));
        fs::write(&target, "bind = SUPER, Q, killactive\n").unwrap();
        let config_file = std::env::temp_dir().join(format!(
            "hyprbind-test-source-persist-config-{}",
            std::process::id()
        ));

        let mut app = edit_app();
        app.config_path = config_file.clone();
        app.edit_buffer = target.display().to_string();
        app.save_source_path();

        let saved = fs::read_to_string(&config_file).unwrap();
        assert!(saved.contains(&format!("source_path = {}", target.display())));

        fs::remove_file(&target).unwrap();
        fs::remove_file(&config_file).unwrap();
    }

    #[test]
    fn save_template_folder_persists_to_config_path() {
        let folder = std::env::temp_dir().join(format!(
            "hyprbind-test-template-persist-folder-{}",
            std::process::id()
        ));
        let config_file = std::env::temp_dir().join(format!(
            "hyprbind-test-template-persist-config-{}",
            std::process::id()
        ));

        let mut app = edit_app();
        app.config_path = config_file.clone();
        app.edit_buffer = folder.display().to_string();
        app.save_template_folder();

        let saved = fs::read_to_string(&config_file).unwrap();
        assert!(saved.contains(&format!("template_folder = {}", folder.display())));

        fs::remove_dir_all(&folder).unwrap();
        fs::remove_file(&config_file).unwrap();
    }

    #[test]
    fn timestamp_string_is_well_formed() {
        let ts = timestamp_string();
        // Either "YYYYMMDD-HHMMSS" (15 chars) from `date`, or raw epoch seconds as a fallback.
        // Either way it should be non-empty and made up only of digits and a possible dash.
        assert!(!ts.is_empty());
        assert!(ts.chars().all(|c| c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn create_backup_copies_the_source_file_byte_for_byte() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-backup-source-{}.conf",
            std::process::id()
        ));
        let backup_dir =
            std::env::temp_dir().join(format!("hyprbind-test-backup-dir-{}", std::process::id()));
        let contents = "$mainMod = SUPER\n# a comment\nbind = $mainMod, Q, killactive\n";
        fs::write(&source, contents).unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.backup_folder = backup_dir.clone();
        app.create_backup();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.starts_with("Backed up to "))
        );

        let entries: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "exactly one backup file should have been created"
        );
        let backup_path = entries[0].path();
        assert_eq!(
            backup_path.extension().and_then(|e| e.to_str()),
            Some(BACKUP_EXTENSION)
        );
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), contents);

        fs::remove_file(&source).unwrap();
        fs::remove_dir_all(&backup_dir).unwrap();
    }

    #[test]
    fn create_backup_does_not_overwrite_an_existing_backup_with_the_same_timestamp() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-backup-collision-source-{}.conf",
            std::process::id()
        ));
        let backup_dir = std::env::temp_dir().join(format!(
            "hyprbind-test-backup-collision-dir-{}",
            std::process::id()
        ));
        fs::create_dir_all(&backup_dir).unwrap();
        fs::write(&source, "bind = $mainMod, Q, killactive\n").unwrap();

        // Simulate a backup that already exists for "now" (as if `b` had just been pressed).
        let stem = source.file_stem().and_then(|s| s.to_str()).unwrap();
        let ts = timestamp_string();
        let existing = backup_dir.join(format!("{stem}-{ts}.{BACKUP_EXTENSION}"));
        fs::write(&existing, "previous backup contents").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.backup_folder = backup_dir.clone();
        app.create_backup();

        assert_eq!(
            fs::read_to_string(&existing).unwrap(),
            "previous backup contents",
            "the earlier backup must survive untouched"
        );
        let entries: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(
            entries.len(),
            2,
            "the new backup should land alongside the old one, not replace it"
        );

        fs::remove_file(&source).unwrap();
        fs::remove_dir_all(&backup_dir).unwrap();
    }

    #[test]
    fn create_backup_reports_failure_when_source_is_unreadable() {
        let missing = std::env::temp_dir().join("hyprbind-test-backup-missing-hopefully.conf");
        let backup_dir = std::env::temp_dir().join(format!(
            "hyprbind-test-backup-dir-missing-{}",
            std::process::id()
        ));

        let mut app = edit_app();
        app.source_path = missing;
        app.backup_folder = backup_dir.clone();
        app.create_backup();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.starts_with("Backup failed"))
        );

        fs::remove_dir_all(&backup_dir).unwrap();
    }

    #[test]
    fn confirm_backup_selection_moves_to_confirm_mode() {
        let mut app = edit_app();
        app.mode = Mode::BackupList;
        app.backup_files = vec![PathBuf::from("/a/one.hbb"), PathBuf::from("/a/two.hbb")];
        app.backup_table_state.select(Some(1));

        app.confirm_backup_selection();

        assert_eq!(app.mode, Mode::BackupConfirm);
        assert_eq!(app.backup_selected_path, Some(PathBuf::from("/a/two.hbb")));
    }

    #[test]
    fn restore_backup_overwrites_source_with_backup_contents_and_reloads() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-restore-source-{}.conf",
            std::process::id()
        ));
        let backup = std::env::temp_dir().join(format!(
            "hyprbind-test-restore-backup-{}.hbb",
            std::process::id()
        ));
        fs::write(&source, "bind = SUPER, Q, killactive\n").unwrap();
        let backup_contents = "bind = SUPER, W, exec, foo\nbind = SUPER, E, exec, bar\n";
        fs::write(&backup, backup_contents).unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.backup_selected_path = Some(backup.clone());
        app.mode = Mode::BackupConfirm;

        app.restore_backup();

        assert_eq!(fs::read_to_string(&source).unwrap(), backup_contents);
        assert_eq!(app.shortcuts.len(), 2);
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.backup_selected_path.is_none());
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.starts_with("Restored "))
        );

        fs::remove_file(&source).unwrap();
        fs::remove_file(&backup).unwrap();
    }

    #[test]
    fn restore_backup_reports_failure_when_backup_is_unreadable() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-restore-bad-source-{}.conf",
            std::process::id()
        ));
        fs::write(&source, "bind = SUPER, Q, killactive\n").unwrap();
        let missing_backup =
            std::env::temp_dir().join("hyprbind-test-restore-missing-backup-hopefully.hbb");

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.backup_selected_path = Some(missing_backup);
        app.mode = Mode::BackupConfirm;

        app.restore_backup();

        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            "bind = SUPER, Q, killactive\n"
        );
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("Couldn't read"))
        );

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn save_backup_folder_persists_to_config_path() {
        let folder = std::env::temp_dir().join(format!(
            "hyprbind-test-backup-folder-persist-{}",
            std::process::id()
        ));
        let config_file = std::env::temp_dir().join(format!(
            "hyprbind-test-backup-folder-persist-config-{}",
            std::process::id()
        ));

        let mut app = edit_app();
        app.config_path = config_file.clone();
        app.edit_buffer = folder.display().to_string();
        app.save_backup_folder();

        let saved = fs::read_to_string(&config_file).unwrap();
        assert!(saved.contains(&format!("backup_folder = {}", folder.display())));

        fs::remove_dir_all(&folder).unwrap();
        fs::remove_file(&config_file).unwrap();
    }

    #[test]
    fn save_terminal_command_persists_to_config_path() {
        let config_file = std::env::temp_dir().join(format!(
            "hyprbind-test-terminal-command-persist-config-{}",
            std::process::id()
        ));

        let mut app = edit_app();
        app.config_path = config_file.clone();
        app.edit_buffer = "alacritty --hold".to_string();
        app.save_terminal_command();

        assert_eq!(app.terminal_command.as_deref(), Some("alacritty --hold"));
        let saved = fs::read_to_string(&config_file).unwrap();
        assert!(saved.contains("terminal_command = alacritty --hold"));

        fs::remove_file(&config_file).unwrap();
    }

    #[test]
    fn save_terminal_command_rejects_empty_input() {
        let mut app = edit_app();
        app.terminal_command = Some("kitty".to_string());
        app.edit_buffer = "   ".to_string();
        app.save_terminal_command();
        assert_eq!(app.terminal_command.as_deref(), Some("kitty"));
        assert!(app.status.is_some());
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
            format: keybindings::SourceFormat::Conf,
            options_raw: None,
        }
    }

    fn shortcut_with_mods(line: usize, mods: &[&str], key: &str) -> Shortcut {
        let mut s = sample_shortcut(line, key);
        s.mods = mods.iter().map(|m| m.to_string()).collect();
        s.mods_raw = mods.join(" ");
        s
    }

    #[test]
    fn save_edit_editkey_no_conflict_saves_normally() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-dupkey-noconflict-{}.conf",
            std::process::id()
        ));
        fs::write(
            &source,
            "bind = $mainMod, Q, exec, foo\nbind = $mainMod, W, exec, foo\n",
        )
        .unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q"), sample_shortcut(2, "W")];
        app.mode = Mode::EditKey;
        app.editing_line = Some(1);
        app.edit_buffer = "SUPER, E".to_string();

        app.save_edit();

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.status.as_deref(), Some("Saved."));
        let contents = fs::read_to_string(&source).unwrap();
        assert!(contents.lines().next().unwrap().contains(", E,"));

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn save_edit_refuses_to_overwrite_a_line_changed_since_load() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-concurrent-writer-{}.conf",
            std::process::id()
        ));
        // Loaded by hyprbind with this content...
        fs::write(&source, "bind = $mainMod, Q, exec, foo\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.mode = Mode::EditKey;
        app.editing_line = Some(1);
        app.edit_buffer = "SUPER, E".to_string();

        // ...then something else (another hyprbind instance, or a hand edit) changes line 1
        // without changing the line count, before this edit is saved.
        fs::write(&source, "bind = $mainMod, Q, exec, somethingelse\n").unwrap();

        app.save_edit();

        assert_eq!(
            app.status.as_deref(),
            Some("Failed to save: source file changed on disk; reload and try again")
        );
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            "bind = $mainMod, Q, exec, somethingelse\n",
            "the concurrent change must survive untouched"
        );

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn save_edit_editkey_on_a_lua_source_rewrites_the_hl_bind_call_in_place() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-lua-editkey-{}.lua",
            std::process::id()
        ));
        fs::write(
            &source,
            "local mainMod = \"SUPER\"\nhl.bind(mainMod .. \" + Q\", hl.dsp.exec_cmd(\"kill.sh\"), { description = \"Kill\" })\n",
        )
        .unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.load();
        assert_eq!(app.shortcuts.len(), 1);
        app.table_state.select_first();

        app.start_edit_key();
        app.edit_buffer = "$mainMod SHIFT, E".to_string();
        app.edit_cursor = app.edit_buffer.chars().count();

        app.save_edit();

        assert_eq!(app.status.as_deref(), Some("Saved."));
        let contents = fs::read_to_string(&source).unwrap();
        assert!(
            contents.contains("hl.bind(mainMod .. \" + SHIFT + E\", hl.dsp.exec_cmd(\"kill.sh\"), { description = \"Kill\" })"),
            "unexpected contents: {contents}"
        );

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn start_edit_description_prefills_an_empty_buffer_for_a_bind_with_none_yet() {
        let mut app = edit_app();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.table_state.select(Some(0));

        app.start_edit_description();

        assert_eq!(app.mode, Mode::EditDescription);
        assert_eq!(app.edit_buffer, "");
    }

    #[test]
    fn start_edit_description_prefills_the_existing_description() {
        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.bind_type = "bindd".to_string();
        s.description_raw = Some("Kill active window".to_string());
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));

        app.start_edit_description();

        assert_eq!(app.mode, Mode::EditDescription);
        assert_eq!(app.edit_buffer, "Kill active window");
    }

    #[test]
    fn start_edit_description_prefills_the_comment_when_there_is_no_description_field() {
        // The common case for real-world .conf files: a plain `bind` with only a trailing
        // comment, which the table already shows in the "Description" column.
        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.comment = Some("Open the browser".to_string());
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));

        app.start_edit_description();

        assert_eq!(app.mode, Mode::EditDescription);
        assert_eq!(app.edit_buffer, "Open the browser");
    }

    #[test]
    fn start_edit_description_works_for_any_conf_bind_type() {
        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.bind_type = "binde".to_string();
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));

        app.start_edit_description();

        assert_eq!(app.mode, Mode::EditDescription);
    }

    #[test]
    fn save_edit_editdescription_sets_a_comment_on_a_plain_bind_without_upgrading_it() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-editdescription-comment-{}.conf",
            std::process::id()
        ));
        fs::write(&source, "bind = $mainMod, Q, exec, foo\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.mode = Mode::EditDescription;
        app.editing_line = Some(1);
        app.edit_buffer = "Kill active window".to_string();

        app.save_edit();

        assert_eq!(app.status.as_deref(), Some("Saved."));
        let contents = fs::read_to_string(&source).unwrap();
        assert_eq!(
            contents,
            "bind = $mainMod, Q, exec, foo # Kill active window\n"
        );

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn save_edit_editdescription_replaces_an_existing_comment() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-editdescription-replace-comment-{}.conf",
            std::process::id()
        ));
        fs::write(&source, "bind = $mainMod, Q, exec, foo # Old comment\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        let mut s = sample_shortcut(1, "Q");
        s.comment = Some("Old comment".to_string());
        s.raw = "bind = $mainMod, Q, exec, foo # Old comment".to_string();
        app.shortcuts = vec![s];
        app.mode = Mode::EditDescription;
        app.editing_line = Some(1);
        app.edit_buffer = "New comment".to_string();

        app.save_edit();

        assert_eq!(app.status.as_deref(), Some("Saved."));
        let contents = fs::read_to_string(&source).unwrap();
        assert_eq!(contents, "bind = $mainMod, Q, exec, foo # New comment\n");

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn save_edit_editdescription_rejects_empty_input_and_leaves_the_file_untouched() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-editdescription-empty-{}.conf",
            std::process::id()
        ));
        fs::write(&source, "bind = SUPER, Q, exec, foo\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.mode = Mode::EditDescription;
        app.editing_line = Some(1);
        app.edit_buffer = "   ".to_string();

        app.save_edit();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("can't be empty"))
        );
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            "bind = SUPER, Q, exec, foo\n"
        );

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn save_edit_editdescription_on_a_lua_source_updates_only_the_options_table() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-editdescription-lua-{}.lua",
            std::process::id()
        ));
        fs::write(
            &source,
            "hl.bind(\"CTRL + ALT + T\", hl.dsp.exec_cmd(\"themes.sh\"), { description = \"Old\" })\n",
        )
        .unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.load();
        assert_eq!(app.shortcuts.len(), 1);
        app.table_state.select_first();

        app.start_edit_description();
        app.edit_buffer = "Open theme picker".to_string();
        app.edit_cursor = app.edit_buffer.chars().count();
        app.save_edit();

        assert_eq!(app.status.as_deref(), Some("Saved."));
        let contents = fs::read_to_string(&source).unwrap();
        assert_eq!(
            contents,
            "hl.bind(\"CTRL + ALT + T\", hl.dsp.exec_cmd(\"themes.sh\"), { description = \"Open theme picker\" })\n"
        );

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn start_template_save_select_is_blocked_for_a_lua_source() {
        let mut app = edit_app();
        app.mode = Mode::Normal;
        app.source_path = PathBuf::from("/some/default.lua");
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.start_template_save_select();
        assert_eq!(app.mode, Mode::Normal);
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("Lua-format"))
        );
    }

    #[test]
    fn apply_template_selection_is_blocked_for_a_lua_source() {
        let mut app = edit_app();
        app.source_path = PathBuf::from("/some/default.lua");
        app.mode = Mode::TemplatePreview;
        app.template_candidates = vec![sample_shortcut(1, "Q")];
        app.template_selected.insert(1);
        app.apply_template_selection();
        assert_eq!(app.mode, Mode::Normal);
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("Lua-format"))
        );
    }

    #[test]
    fn open_terminal_at_script_reports_a_non_exec_shortcut() {
        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.dispatcher = "killactive".to_string();
        s.args = String::new();
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));

        app.open_terminal_at_script();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("doesn't run exec"))
        );
    }

    #[test]
    fn open_terminal_at_script_reports_no_resolvable_script() {
        let mut app = edit_app();
        app.shortcuts = vec![sample_shortcut(1, "Q")]; // dispatcher "exec", args "foo" (not a real file)
        app.table_state.select(Some(0));

        app.open_terminal_at_script();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("couldn't find a script file"))
        );
    }

    #[test]
    fn open_terminal_at_script_reports_no_terminal_command_set() {
        let script = std::env::temp_dir().join(format!(
            "hyprbind-test-open-terminal-noterm-{}.sh",
            std::process::id()
        ));
        fs::write(&script, "echo hi\n").unwrap();

        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.args = script.display().to_string();
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));
        app.terminal_command = None;

        app.open_terminal_at_script();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("No terminal command set"))
        );

        fs::remove_file(&script).unwrap();
    }

    #[test]
    fn open_terminal_at_script_spawns_the_terminal_command_at_the_scripts_directory() {
        let script = std::env::temp_dir().join(format!(
            "hyprbind-test-open-terminal-ok-{}.sh",
            std::process::id()
        ));
        fs::write(&script, "echo hi\n").unwrap();

        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.args = script.display().to_string();
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));
        // "true" is a real, universally-present no-op binary, so this doesn't need an actual
        // terminal emulator or a display to run headlessly in a test.
        app.terminal_command = Some("true".to_string());

        app.open_terminal_at_script();

        let expected_dir = script.parent().unwrap().display().to_string();
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s == format!("Opened a terminal at {expected_dir}.")),
            "unexpected status: {:?}",
            app.status
        );

        fs::remove_file(&script).unwrap();
    }

    #[test]
    fn save_edit_editkey_unchanged_combo_is_not_flagged_as_a_self_conflict() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-dupkey-selfsame-{}.conf",
            std::process::id()
        ));
        fs::write(&source, "bind = $mainMod, Q, exec, foo\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.mode = Mode::EditKey;
        app.editing_line = Some(1);
        app.edit_buffer = "SUPER, Q".to_string();

        app.save_edit();

        assert_eq!(
            app.mode,
            Mode::Normal,
            "saving a shortcut's own unchanged combo isn't a conflict"
        );
        assert_eq!(app.status.as_deref(), Some("Saved."));

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn save_edit_editkey_conflict_offers_a_fix_when_one_is_available() {
        let mut app = edit_app();
        app.shortcuts = vec![
            shortcut_with_mods(1, &["SUPER"], "Q"),
            shortcut_with_mods(2, &["SUPER"], "W"),
        ];
        app.mode = Mode::EditKey;
        app.editing_line = Some(1);
        app.edit_buffer = "SUPER, W".to_string();

        app.save_edit();

        assert_eq!(app.mode, Mode::DuplicateKeyConfirm);
        assert_eq!(app.duplicate_conflict_line, Some(2));
        assert_eq!(app.duplicate_attempted_combo, "SUPER + W");
        assert_eq!(
            app.duplicate_fix_display.as_deref(),
            Some("SUPER + SHIFT + W")
        );
    }

    #[test]
    fn save_edit_editkey_conflict_with_no_available_fix() {
        let mut app = edit_app();
        app.shortcuts = vec![
            shortcut_with_mods(1, &["ALT"], "Q"),
            shortcut_with_mods(2, &["SUPER"], "Q"),
            shortcut_with_mods(3, &["SUPER", "SHIFT"], "Q"),
            shortcut_with_mods(4, &["SUPER", "CTRL"], "Q"),
            shortcut_with_mods(5, &["SUPER", "ALT"], "Q"),
        ];
        app.mode = Mode::EditKey;
        app.editing_line = Some(1);
        app.edit_buffer = "SUPER, Q".to_string();

        app.save_edit();

        assert_eq!(app.mode, Mode::DuplicateKeyConfirm);
        assert_eq!(app.duplicate_conflict_line, Some(2));
        assert!(
            app.duplicate_fix_display.is_none(),
            "every SUPER+<one more modifier>+Q combo is already taken"
        );
    }

    #[test]
    fn save_edit_editkey_conflict_detection_resolves_dollar_vars() {
        let mut app = edit_app();
        app.variables = vec![Variable {
            name: "mainMod".to_string(),
            value: "SUPER".to_string(),
            line: 1,
            format: SourceFormat::Conf,
            comment: None,
            raw: "$mainMod = SUPER".to_string(),
        }];
        app.shortcuts = vec![
            shortcut_with_mods(2, &["SUPER"], "Q"),
            shortcut_with_mods(3, &["SUPER"], "W"),
        ];
        app.mode = Mode::EditKey;
        app.editing_line = Some(2);
        app.edit_buffer = "$mainMod, W".to_string();

        app.save_edit();

        assert_eq!(app.mode, Mode::DuplicateKeyConfirm);
        assert_eq!(app.duplicate_conflict_line, Some(3));
        assert_eq!(app.duplicate_attempted_combo, "SUPER + W");
    }

    #[test]
    fn accept_duplicate_fix_writes_the_fixed_combo_and_returns_to_normal() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-dupkey-acceptfix-{}.conf",
            std::process::id()
        ));
        fs::write(&source, "bind = $mainMod, Q, exec, foo\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![shortcut_with_mods(1, &["SUPER"], "Q")];
        app.mode = Mode::DuplicateKeyConfirm;
        app.editing_line = Some(1);
        app.duplicate_fix_mods_raw = Some("SUPER SHIFT".to_string());
        app.duplicate_key_raw = "Q".to_string();

        app.accept_duplicate_fix();

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.duplicate_fix_mods_raw.is_none());
        let contents = fs::read_to_string(&source).unwrap();
        assert!(contents.starts_with("bind = SUPER SHIFT, Q,"));

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn accept_duplicate_fix_with_nothing_to_accept_just_cancels() {
        let mut app = edit_app();
        app.shortcuts = vec![shortcut_with_mods(1, &["SUPER"], "Q")];
        app.mode = Mode::DuplicateKeyConfirm;
        app.editing_line = Some(1);
        app.duplicate_fix_mods_raw = None;
        app.duplicate_conflict_line = Some(2);

        app.accept_duplicate_fix();

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.editing_line.is_none());
        assert!(app.duplicate_conflict_line.is_none());
    }

    #[test]
    fn cancel_duplicate_confirm_writes_nothing_and_resets_state() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-dupkey-cancel-{}.conf",
            std::process::id()
        ));
        let original = "bind = SUPER, Q, exec, foo\n";
        fs::write(&source, original).unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![shortcut_with_mods(1, &["SUPER"], "Q")];
        app.mode = Mode::DuplicateKeyConfirm;
        app.editing_line = Some(1);
        app.duplicate_fix_mods_raw = Some("SUPER SHIFT".to_string());
        app.duplicate_conflict_line = Some(2);

        app.cancel_duplicate_confirm();

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.editing_line.is_none());
        assert!(app.duplicate_fix_mods_raw.is_none());
        assert!(app.duplicate_conflict_line.is_none());
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            original,
            "cancel must never write anything"
        );

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn start_add_shortcut_is_blocked_for_a_lua_source() {
        let mut app = edit_app();
        app.mode = Mode::Normal;
        app.source_path = PathBuf::from("/some/default.lua");
        app.start_add_shortcut();
        assert_eq!(app.mode, Mode::Normal);
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("Lua-format"))
        );
    }

    #[test]
    fn start_add_shortcut_enters_add_mode_with_an_empty_buffer() {
        let mut app = edit_app();
        app.mode = Mode::Normal;
        app.source_path = PathBuf::from("/some/default.conf");
        app.edit_buffer = "leftover".to_string();
        app.start_add_shortcut();
        assert_eq!(app.mode, Mode::AddShortcut);
        assert_eq!(app.edit_buffer, "");
    }

    #[test]
    fn save_new_shortcut_rejects_empty_input_and_leaves_the_file_untouched() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-add-empty-{}.conf",
            std::process::id()
        ));
        fs::write(&source, "bind = SUPER, Q, exec, foo\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.mode = Mode::AddShortcut;
        app.edit_buffer = "   ".to_string();

        app.save_new_shortcut();

        assert_eq!(app.mode, Mode::AddShortcut);
        assert_eq!(app.status.as_deref(), Some("Description can't be empty."));
        let contents = fs::read_to_string(&source).unwrap();
        assert_eq!(contents, "bind = SUPER, Q, exec, foo\n");

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn save_new_shortcut_appends_a_placeholder_bind_and_selects_it() {
        let source =
            std::env::temp_dir().join(format!("hyprbind-test-add-new-{}.conf", std::process::id()));
        fs::write(&source, "bind = SUPER, Q, exec, foo\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.mode = Mode::AddShortcut;
        app.edit_buffer = "Launch the launcher".to_string();

        app.save_new_shortcut();

        assert_eq!(app.mode, Mode::Normal);
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.starts_with("Added."))
        );

        let contents = fs::read_to_string(&source).unwrap();
        assert!(contents.contains("bind = , CHANGEME, exec # Launch the launcher"));

        let selected = app
            .table_state
            .selected()
            .and_then(|idx| app.visible().get(idx).copied());
        assert!(selected.is_some_and(|s| s.comment.as_deref() == Some("Launch the launcher")));

        fs::remove_file(&source).unwrap();
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
        let path =
            std::env::temp_dir().join(format!("hyprbind-test-append-{}.conf", std::process::id()));
        fs::write(&path, "bind = $mainMod, Q, killactive\n").unwrap();

        append_lines(
            &path,
            "# Applied from template: gaming.hbt",
            &["bind = SUPER, W, exec, foo".to_string()],
        )
        .unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            "bind = $mainMod, Q, killactive\n\n# Applied from template: gaming.hbt\nbind = SUPER, W, exec, foo\n"
        );

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn list_files_with_extension_filters_and_sorts() {
        let dir = std::env::temp_dir().join(format!("hyprbind-test-list-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("b.hbt"), "").unwrap();
        fs::write(dir.join("a.hbt"), "").unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();
        fs::write(dir.join("c.hbb"), "").unwrap();

        assert_eq!(
            list_files_with_extension(&dir, TEMPLATE_EXTENSION),
            vec![dir.join("a.hbt"), dir.join("b.hbt")]
        );
        assert_eq!(
            list_files_with_extension(&dir, BACKUP_EXTENSION),
            vec![dir.join("c.hbb")]
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_files_with_extension_on_missing_folder_returns_empty() {
        let missing = std::env::temp_dir().join("hyprbind-does-not-exist-hopefully");
        assert!(list_files_with_extension(&missing, TEMPLATE_EXTENSION).is_empty());
    }
}
