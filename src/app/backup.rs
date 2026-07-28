use std::fs;
use std::path::{Path, PathBuf};

use ratatui::widgets::TableState;

use crate::fs_util::write_atomic;

use super::lines::list_files_with_extension;
use super::paths::expand_home;
use super::{App, Mode};

pub(super) const BACKUP_EXTENSION: &str = "hbb";

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

impl App {
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
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{edit_app, sample_shortcut};
    use super::*;

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
}
