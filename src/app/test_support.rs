#![cfg(test)]

use std::collections::HashSet;
use std::path::PathBuf;

use ratatui::widgets::TableState;

use crate::keybindings::{self, Shortcut};

use super::{App, Mode};

pub(super) fn edit_app() -> App {
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

pub(super) fn sample_shortcut(line: usize, key: &str) -> Shortcut {
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

pub(super) fn shortcut_with_mods(line: usize, mods: &[&str], key: &str) -> Shortcut {
    let mut s = sample_shortcut(line, key);
    s.mods = mods.iter().map(|m| m.to_string()).collect();
    s.mods_raw = mods.join(" ");
    s
}
