use crate::keybindings::SourceFormat;

use super::lines::{append_lines, delete_line};
use super::{App, Mode};

/// Placeholder key a freshly-added shortcut (see `App::save_new_shortcut`) is bound to until `e`
/// replaces it: not a real Hyprland keysym, so Hyprland's own reload never actually binds
/// anything to it in the meantime.
const NEW_SHORTCUT_KEY: &str = "CHANGEME";

/// Placeholder dispatcher a freshly-added shortcut is bound to until `a` replaces it.
const NEW_SHORTCUT_DISPATCHER: &str = "exec";

impl App {
    /// Ask for confirmation before deleting the selected shortcut; see `confirm_delete_shortcut`.
    pub fn start_delete_shortcut(&mut self) {
        let Some(idx) = self.table_state.selected() else {
            return;
        };
        let Some(line) = self.visible().get(idx).map(|s| s.line) else {
            return;
        };
        self.clear_status();
        self.editing_line = Some(line);
        self.mode = Mode::DeleteConfirm;
    }

    /// Abandon a pending delete without touching the file.
    pub fn cancel_delete_shortcut(&mut self) {
        self.editing_line = None;
        self.mode = Mode::Normal;
    }

    /// Remove the shortcut pending confirmation (see `start_delete_shortcut`) from `source_path`,
    /// report the result, and return to `Mode::Normal`. Keeps the selection at roughly the same
    /// row it was on, rather than jumping back to the top of the list, so deleting several
    /// shortcuts in a row doesn't mean re-scrolling each time.
    pub fn confirm_delete_shortcut(&mut self) {
        let Some(line_no) = self.editing_line else {
            self.mode = Mode::Normal;
            return;
        };
        let selected_idx = self.table_state.selected().unwrap_or(0);
        let expected_old_line = self.expected_line_at(line_no).map(str::to_string);

        match delete_line(&self.source_path, line_no, expected_old_line.as_deref()) {
            Ok(()) => {
                self.set_status("Deleted.".to_string());
                self.load();
                let visible_len = self.visible().len();
                if visible_len == 0 {
                    self.table_state.select(None);
                } else {
                    self.table_state
                        .select(Some(selected_idx.min(visible_len - 1)));
                }
            }
            Err(err) => {
                self.set_status(format!("Failed to delete: {err}"));
            }
        }

        self.editing_line = None;
        self.mode = Mode::Normal;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    use super::super::test_support::{edit_app, sample_shortcut};

    #[test]
    fn start_delete_shortcut_enters_confirm_mode_for_the_selected_row() {
        let mut app = edit_app();
        app.mode = Mode::Normal;
        app.shortcuts = vec![sample_shortcut(1, "Q"), sample_shortcut(2, "W")];
        app.table_state.select(Some(1));

        app.start_delete_shortcut();

        assert_eq!(app.mode, Mode::DeleteConfirm);
        assert_eq!(app.editing_line, Some(2));
    }

    #[test]
    fn confirm_delete_shortcut_removes_the_line_and_keeps_a_nearby_row_selected() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-delete-confirm-{}.conf",
            std::process::id()
        ));
        fs::write(
                &source,
                "bind = $mainMod, Q, exec, foo\nbind = $mainMod, W, exec, foo\nbind = $mainMod, E, exec, foo\n",
            )
            .unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![
            sample_shortcut(1, "Q"),
            sample_shortcut(2, "W"),
            sample_shortcut(3, "E"),
        ];
        app.mode = Mode::DeleteConfirm;
        app.editing_line = Some(2);
        app.table_state.select(Some(1));

        app.confirm_delete_shortcut();

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.status.as_deref(), Some("Deleted."));
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            "bind = $mainMod, Q, exec, foo\nbind = $mainMod, E, exec, foo\n"
        );
        // Two shortcuts remain (indices 0 and 1); the deleted row was at index 1, so the
        // selection should land on whatever is now at index 1 (the former third row) rather
        // than jumping back to the top.
        assert_eq!(app.table_state.selected(), Some(1));
        assert!(app.editing_line.is_none());

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn confirm_delete_shortcut_refuses_when_the_line_changed_since_load() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-delete-concurrent-{}.conf",
            std::process::id()
        ));
        fs::write(&source, "bind = $mainMod, Q, exec, foo\n").unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.mode = Mode::DeleteConfirm;
        app.editing_line = Some(1);
        app.table_state.select(Some(0));

        // Something else changes the line before the delete is confirmed.
        fs::write(&source, "bind = $mainMod, Q, exec, somethingelse\n").unwrap();

        app.confirm_delete_shortcut();

        assert_eq!(
            app.status.as_deref(),
            Some("Failed to delete: source file changed on disk; reload and try again")
        );
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            "bind = $mainMod, Q, exec, somethingelse\n",
            "the concurrent change must survive untouched"
        );

        fs::remove_file(&source).unwrap();
    }

    #[test]
    fn cancel_delete_shortcut_writes_nothing_and_resets_state() {
        let source = std::env::temp_dir().join(format!(
            "hyprbind-test-delete-cancel-{}.conf",
            std::process::id()
        ));
        let original = "bind = $mainMod, Q, exec, foo\n";
        fs::write(&source, original).unwrap();

        let mut app = edit_app();
        app.source_path = source.clone();
        app.shortcuts = vec![sample_shortcut(1, "Q")];
        app.mode = Mode::DeleteConfirm;
        app.editing_line = Some(1);

        app.cancel_delete_shortcut();

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.editing_line.is_none());
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
}
