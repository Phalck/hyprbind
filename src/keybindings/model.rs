/// A single parsed keybinding line, e.g. `bind = $mainMod, Q, killactive # Kill active window`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// The raw directive that introduced this bind: "bind", "bindd", "binde", "bindm", "bindle", ...
    pub bind_type: String,
    /// Modifier keys, variables already resolved (e.g. "SUPER" rather than "$mainMod").
    pub mods: Vec<String>,
    pub key: String,
    /// Only present for `bindd` lines, which carry an explicit description field.
    pub description: Option<String>,
    pub dispatcher: String,
    pub args: String,
    /// Trailing `# ...` comment, if any.
    pub comment: Option<String>,
    /// 1-based line number in the source file, used to write an edited line back in place.
    pub line: usize,
    /// The exact, unmodified source line this shortcut was parsed from (no `$VAR` substitution
    /// applied). Used as the starting point when editing, so a save round-trips anything the
    /// parser doesn't otherwise represent (comments, variable references, exact spacing).
    pub raw: String,
}

impl Shortcut {
    /// Human-readable key combo, e.g. "SUPER + SHIFT + Q".
    pub fn key_combo(&self) -> String {
        if self.mods.is_empty() {
            self.key.clone()
        } else {
            format!("{} + {}", self.mods.join(" + "), self.key)
        }
    }

    /// Human-readable action, e.g. "exec ~/.config/ml4w/settings/terminal.sh".
    pub fn action(&self) -> String {
        if self.args.is_empty() {
            self.dispatcher.clone()
        } else {
            format!("{} {}", self.dispatcher, self.args)
        }
    }

    /// Best available label: explicit description, falling back to the trailing comment.
    pub fn label(&self) -> &str {
        self.description
            .as_deref()
            .or(self.comment.as_deref())
            .unwrap_or("")
    }

    /// Whether this shortcut matches a search query.
    ///
    /// `query_lower` must already be lowercased; this is the caller's responsibility so a
    /// multi-item search doesn't re-lowercase the same query on every shortcut.
    pub fn matches(&self, query_lower: &str) -> bool {
        if query_lower.is_empty() {
            return true;
        }
        self.key_combo().to_lowercase().contains(query_lower)
            || self.action().to_lowercase().contains(query_lower)
            || self.label().to_lowercase().contains(query_lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut() -> Shortcut {
        Shortcut {
            bind_type: "bind".to_string(),
            mods: vec!["SUPER".to_string(), "SHIFT".to_string()],
            key: "A".to_string(),
            description: None,
            dispatcher: "exec".to_string(),
            args: "~/.config/hypr/scripts/toggle-animations.sh".to_string(),
            comment: Some("Toggle animations".to_string()),
            line: 1,
            raw: "bind = $mainMod SHIFT, A, exec, $HYPRSCRIPTS/toggle-animations.sh # Toggle animations"
                .to_string(),
        }
    }

    #[test]
    fn matches_key_combo_case_insensitively() {
        assert!(shortcut().matches("shift"));
        assert!(shortcut().matches("super + shift + a"));
    }

    #[test]
    fn matches_action_and_label() {
        assert!(shortcut().matches("toggle-animations"));
        assert!(shortcut().matches("toggle animations"));
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(shortcut().matches(""));
    }

    #[test]
    fn no_match_returns_false() {
        assert!(!shortcut().matches("nonexistent"));
    }
}
