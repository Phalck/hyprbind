use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs_util::write_atomic;

/// Settings that persist between runs, stored as simple `key = value` lines (not a real config
/// format like TOML — there are only a handful of settings, and the app already hand-rolls a
/// parser for Hyprland's own `key = value`-shaped syntax, so a second one here is a handful of
/// lines).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Settings {
    pub source_path: Option<PathBuf>,
    pub template_folder: Option<PathBuf>,
    pub backup_folder: Option<PathBuf>,
    /// The program (and any fixed arguments) used to open a terminal, e.g. "kitty" or
    /// "alacritty --hold". A `String` rather than a `PathBuf`: it can carry extra arguments, and
    /// is usually just a bare program name resolved against `$PATH`, not a path of its own.
    pub terminal_command: Option<String>,
}

/// `~/.config/hyprbind/config`.
pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config/hyprbind/config")
}

/// Load persisted settings from `path`. Missing file, unreadable file, or a file with neither
/// setting all mean the same thing: "nothing persisted yet" — return `Settings::default()`
/// rather than failing, so a missing config file is never an error the user has to deal with.
pub fn load_from(path: &Path) -> Settings {
    let Ok(contents) = fs::read_to_string(path) else {
        return Settings::default();
    };
    parse(&contents)
}

fn parse(contents: &str) -> Settings {
    let mut settings = Settings::default();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "source_path" => settings.source_path = Some(PathBuf::from(value)),
            "template_folder" => settings.template_folder = Some(PathBuf::from(value)),
            "backup_folder" => settings.backup_folder = Some(PathBuf::from(value)),
            "terminal_command" => settings.terminal_command = Some(value.to_string()),
            _ => {}
        }
    }
    settings
}

/// Write `settings` to `path`, creating its parent directory first if needed, atomically.
pub fn save_to(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut contents = String::new();
    if let Some(source_path) = &settings.source_path {
        contents.push_str(&format!("source_path = {}\n", source_path.display()));
    }
    if let Some(template_folder) = &settings.template_folder {
        contents.push_str(&format!(
            "template_folder = {}\n",
            template_folder.display()
        ));
    }
    if let Some(backup_folder) = &settings.backup_folder {
        contents.push_str(&format!("backup_folder = {}\n", backup_folder.display()));
    }
    if let Some(terminal_command) = &settings.terminal_command {
        contents.push_str(&format!("terminal_command = {terminal_command}\n"));
    }

    write_atomic(path, &contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "hyprbind-config-test-{name}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn parse_reads_all_keys_and_ignores_comments_and_blanks() {
        let contents = "\
# a comment
source_path = /home/me/.config/hypr/binds.conf

template_folder = /home/me/Templates
backup_folder = /home/me/Backups
terminal_command = kitty --hold
";
        let settings = parse(contents);
        assert_eq!(
            settings.source_path,
            Some(PathBuf::from("/home/me/.config/hypr/binds.conf"))
        );
        assert_eq!(
            settings.template_folder,
            Some(PathBuf::from("/home/me/Templates"))
        );
        assert_eq!(
            settings.backup_folder,
            Some(PathBuf::from("/home/me/Backups"))
        );
        assert_eq!(settings.terminal_command, Some("kitty --hold".to_string()));
    }

    #[test]
    fn parse_ignores_unknown_keys_and_malformed_lines() {
        let contents = "not a key value line\nunknown_key = whatever\nsource_path = /a/b\n";
        let settings = parse(contents);
        assert_eq!(settings.source_path, Some(PathBuf::from("/a/b")));
        assert_eq!(settings.template_folder, None);
    }

    #[test]
    fn load_from_missing_file_returns_default() {
        let missing = scratch_path("missing-hopefully");
        assert_eq!(load_from(&missing), Settings::default());
    }

    #[test]
    fn save_to_then_load_from_round_trips() {
        let dir = scratch_path("roundtrip-dir");
        let path = dir.join("config");
        let settings = Settings {
            source_path: Some(PathBuf::from("/a/b/binds.conf")),
            template_folder: Some(PathBuf::from("/a/b/templates")),
            backup_folder: Some(PathBuf::from("/a/b/backups")),
            terminal_command: Some("alacritty".to_string()),
        };

        save_to(&path, &settings).unwrap();
        assert_eq!(load_from(&path), settings);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_to_creates_the_parent_directory() {
        let dir = scratch_path("creates-dir");
        let path = dir.join("nested/config");
        let settings = Settings {
            source_path: Some(PathBuf::from("/x")),
            template_folder: None,
            backup_folder: None,
            terminal_command: None,
        };

        save_to(&path, &settings).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("tmp").exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_to_only_writes_settings_that_are_present() {
        let dir = scratch_path("partial");
        let path = dir.join("config");
        let settings = Settings {
            source_path: Some(PathBuf::from("/only/source")),
            template_folder: None,
            backup_folder: None,
            terminal_command: None,
        };

        save_to(&path, &settings).unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("source_path = /only/source"));
        assert!(!contents.contains("template_folder"));

        fs::remove_dir_all(&dir).unwrap();
    }
}
