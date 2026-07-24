mod discover;
mod lua_parser;
mod model;
mod parser;

use std::io;
use std::path::Path;

use crate::fs_util::read_to_string_capped;

pub use discover::discover;
pub use model::{ParsedConfig, Shortcut, SourceFormat, Variable};

/// Parse a Hyprland keybindings file, picking the syntax (`.conf` or Lua) by its extension.
pub fn parse_file(path: impl AsRef<Path>) -> io::Result<ParsedConfig> {
    let path = path.as_ref();
    let contents = read_to_string_capped(path)?;
    Ok(match format_for_path(path) {
        SourceFormat::Lua => lua_parser::parse_str(&contents),
        SourceFormat::Conf => parser::parse_str(&contents),
    })
}

/// Which keybinding syntax `path` should be parsed as: Lua for a `.lua` extension, `.conf`
/// syntax for anything else (including hyprbind's own `.hbt`/`.hbb` files, which are always
/// written in `.conf` syntax regardless of the source file they were saved from).
pub fn format_for_path(path: &Path) -> SourceFormat {
    if path.extension().and_then(|ext| ext.to_str()) == Some("lua") {
        SourceFormat::Lua
    } else {
        SourceFormat::Conf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_refuses_a_file_over_the_size_cap() {
        let path = std::env::temp_dir()
            .join(format!("hyprbind-test-parse-file-huge-{}.conf", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(17 * 1024 * 1024).unwrap();

        assert!(parse_file(&path).is_err());

        std::fs::remove_file(&path).unwrap();
    }
}
