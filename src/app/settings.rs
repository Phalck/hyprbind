use crate::keybindings::{self, SourceFormat};

use super::paths::expand_home;
use super::{App, Mode};

impl App {
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
    pub(super) fn source_format(&self) -> SourceFormat {
        keybindings::format_for_path(&self.source_path)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use super::super::test_support::{edit_app, sample_shortcut};

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
}
