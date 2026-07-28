use std::path::PathBuf;

pub(super) fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
}

/// Where the active Hyprland keybinding set lives, per the ML4W dotfiles layout.
pub(super) fn default_keybindings_path() -> PathBuf {
    home_dir().join(".mydotfiles/com.ml4w.dotfiles/.config/hypr/conf/keybindings/default.conf")
}

/// Expand a leading `~` (a bare `~` or `~/...`) against `$HOME`. Any other input is used as-is.
pub(super) fn expand_home(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        home_dir().join(rest)
    } else if input == "~" {
        home_dir()
    } else {
        PathBuf::from(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expand_home_handles_tilde_forms() {
        let home = home_dir();
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/Templates"), home.join("Templates"));
        assert_eq!(expand_home("/etc/foo"), PathBuf::from("/etc/foo"));
    }
}
