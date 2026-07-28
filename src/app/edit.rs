use crate::keybindings::SourceFormat;

use super::lines::write_line;
use super::{App, Mode, Shortcut};

/// Modifiers tried, in order, as a candidate fix when an edited key combo collides with another
/// shortcut's. Standard Hyprland modifier names, not anything ML4W- or config-specific.
const STANDARD_MODIFIERS: [&str; 4] = ["SUPER", "SHIFT", "CTRL", "ALT"];

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

impl App {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::keybindings::Variable;

    use super::super::test_support::{edit_app, sample_shortcut, shortcut_with_mods};

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
}
