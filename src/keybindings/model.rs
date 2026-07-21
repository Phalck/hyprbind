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
    /// Trailing `# ...` comment, if any. Never `$VAR`-substituted.
    pub comment: Option<String>,
    /// 1-based line number in the source file, used to write an edited line back in place.
    pub line: usize,
    /// The exact, unmodified source line this shortcut was parsed from.
    pub raw: String,

    /// The mods field exactly as written, e.g. "$mainMod SHIFT" rather than "SUPER SHIFT".
    pub mods_raw: String,
    pub key_raw: String,
    pub description_raw: Option<String>,
    pub dispatcher_raw: String,
    pub args_raw: String,
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

    /// Starting text for the "edit key" text box: the mods and key fields exactly as written,
    /// comma-separated in source order.
    pub fn key_edit_buffer(&self) -> String {
        format!("{}, {}", self.mods_raw, self.key_raw)
    }

    /// Starting text for the "edit target" text box: the dispatcher and its arguments exactly as
    /// written, comma-separated. Arguments are omitted if the original line had none.
    pub fn target_edit_buffer(&self) -> String {
        if self.args_raw.is_empty() {
            self.dispatcher_raw.clone()
        } else {
            format!("{}, {}", self.dispatcher_raw, self.args_raw)
        }
    }

    /// Rebuild the source line with the mods/key field replaced, everything else (description,
    /// dispatcher, args, comment) kept exactly as originally written.
    pub fn with_key(&self, mods_raw: &str, key_raw: &str) -> String {
        self.build_line(mods_raw, key_raw, &self.dispatcher_raw, &self.args_raw)
    }

    /// Rebuild the source line with the dispatcher/args field replaced, everything else (mods,
    /// key, description, comment) kept exactly as originally written.
    pub fn with_target(&self, dispatcher_raw: &str, args_raw: &str) -> String {
        self.build_line(&self.mods_raw, &self.key_raw, dispatcher_raw, args_raw)
    }

    /// Rebuild the full source line from its fields, normalizing separators to a consistent
    /// `field, field, ... # comment` style. This discards any original column-alignment padding
    /// around commas or before the comment; field content itself is always preserved exactly.
    fn build_line(&self, mods_raw: &str, key_raw: &str, dispatcher_raw: &str, args_raw: &str) -> String {
        let mut fields = vec![mods_raw.to_string(), key_raw.to_string()];
        if let Some(description) = &self.description_raw {
            fields.push(description.clone());
        }
        fields.push(dispatcher_raw.to_string());
        if !args_raw.is_empty() {
            fields.push(args_raw.to_string());
        }

        let mut line = format!("{} = {}", self.bind_type, fields.join(", "));
        if let Some(comment) = &self.comment {
            line.push_str(" # ");
            line.push_str(comment);
        }
        line
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
            mods_raw: "$mainMod SHIFT".to_string(),
            key_raw: "A".to_string(),
            description_raw: None,
            dispatcher_raw: "exec".to_string(),
            args_raw: "$HYPRSCRIPTS/toggle-animations.sh".to_string(),
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

    #[test]
    fn key_edit_buffer_uses_raw_unsubstituted_mods() {
        assert_eq!(shortcut().key_edit_buffer(), "$mainMod SHIFT, A");
    }

    #[test]
    fn target_edit_buffer_uses_raw_unsubstituted_args() {
        assert_eq!(
            shortcut().target_edit_buffer(),
            "exec, $HYPRSCRIPTS/toggle-animations.sh"
        );
    }

    #[test]
    fn target_edit_buffer_omits_empty_args() {
        let mut s = shortcut();
        s.args_raw = String::new();
        s.dispatcher_raw = "killactive".to_string();
        assert_eq!(s.target_edit_buffer(), "killactive");
    }

    #[test]
    fn with_key_replaces_only_mods_and_key() {
        let updated = shortcut().with_key("$mainMod CTRL", "B");
        assert_eq!(
            updated,
            "bind = $mainMod CTRL, B, exec, $HYPRSCRIPTS/toggle-animations.sh # Toggle animations"
        );
    }

    #[test]
    fn with_target_replaces_only_dispatcher_and_args_and_preserves_var_in_mods() {
        let updated = shortcut().with_target("exec", "~/new-script.sh");
        assert_eq!(
            updated,
            "bind = $mainMod SHIFT, A, exec, ~/new-script.sh # Toggle animations"
        );
    }

    #[test]
    fn with_target_drops_empty_args_field() {
        let updated = shortcut().with_target("killactive", "");
        assert_eq!(
            updated,
            "bind = $mainMod SHIFT, A, killactive # Toggle animations"
        );
    }

    #[test]
    fn with_key_preserves_bindd_description_field() {
        let mut s = shortcut();
        s.bind_type = "bindd".to_string();
        s.description_raw = Some("Float all windows".to_string());
        let updated = s.with_key("$mainMod SHIFT", "T");
        assert_eq!(
            updated,
            "bindd = $mainMod SHIFT, T, Float all windows, exec, $HYPRSCRIPTS/toggle-animations.sh # Toggle animations"
        );
    }
}
