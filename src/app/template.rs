use std::fs;

use ratatui::widgets::TableState;

use crate::keybindings::{self, SourceFormat};

use super::lines::{append_lines, list_files_with_extension, write_template};
use super::paths::expand_home;
use super::{App, Mode};

pub(super) const TEMPLATE_EXTENSION: &str = "hbt";

impl App {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use super::super::test_support::{edit_app, sample_shortcut};

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
}
