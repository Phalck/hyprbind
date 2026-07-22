/// Which keybinding file syntax a `Shortcut` or `Variable` was parsed from, since the two
/// supported formats (Hyprland's own `.conf` and ML4W's Lua DSL) need different logic to
/// rebuild a source line from edited fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    /// Hyprland's native `bind = mods, key, dispatcher, args # comment` syntax.
    Conf,
    /// ML4W's `hl.bind(key_expr, dispatcher_expr, { options })` Lua syntax.
    Lua,
}

/// A `$VAR = value` definition line (`.conf`) or `local NAME = "value"` assignment (Lua), e.g.
/// `$mainMod = SUPER` or `local mainMod = "SUPER"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    /// Without the leading `$` (`.conf`) or the `local` keyword (Lua), e.g. "mainMod".
    pub name: String,
    pub value: String,
    /// 1-based line number in the source file, used to write an edited value back in place.
    pub line: usize,
    pub format: SourceFormat,
    /// Trailing `-- ...` comment on the definition line. Only ever populated for `Lua`
    /// variables; `.conf` `$VAR = value` lines have no comment syntax of their own.
    pub comment: Option<String>,
}

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

    /// The mods field exactly as written, e.g. "$mainMod SHIFT" rather than "SUPER SHIFT". For a
    /// `Lua`-format shortcut, a variable reference (e.g. Lua's `mainMod ..`) is written using the
    /// same `$mainMod`-style marker as `.conf`, even though the source syntax has no `$` of its
    /// own; it's hyprbind's own convention, kept consistent across both formats so editing looks
    /// and works the same regardless of which file a shortcut came from.
    pub mods_raw: String,
    pub key_raw: String,
    pub description_raw: Option<String>,
    pub dispatcher_raw: String,
    pub args_raw: String,

    pub format: SourceFormat,
    /// `Lua`-format only: the raw text of `hl.bind`'s third argument (the options table),
    /// verbatim including its braces, e.g. `{ mouse = true, description = "..." }`. Neither `e`
    /// nor `a` editing touches this, so it's carried through unchanged on every rebuild. `None`
    /// for `.conf` shortcuts, and for a Lua `hl.bind` call with no options table at all.
    pub options_raw: Option<String>,
}

/// The result of parsing a Hyprland keybindings file (either syntax): its shortcuts plus the
/// variable definitions used to resolve them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedConfig {
    pub shortcuts: Vec<Shortcut>,
    pub variables: Vec<Variable>,
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

    /// Rebuild the full source line from its fields. Dispatches on `format`, since `.conf` and
    /// Lua need entirely different syntax to say the same thing.
    fn build_line(&self, mods_raw: &str, key_raw: &str, dispatcher_raw: &str, args_raw: &str) -> String {
        match self.format {
            SourceFormat::Conf => self.build_conf_line(mods_raw, key_raw, dispatcher_raw, args_raw),
            SourceFormat::Lua => self.build_lua_line(mods_raw, key_raw, dispatcher_raw),
        }
    }

    /// Rebuild a `.conf` source line, normalizing separators to a consistent
    /// `field, field, ... # comment` style. This discards any original column-alignment padding
    /// around commas or before the comment; field content itself is always preserved exactly.
    fn build_conf_line(&self, mods_raw: &str, key_raw: &str, dispatcher_raw: &str, args_raw: &str) -> String {
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

    /// Rebuild an `hl.bind(...)` source line. `dispatcher_raw` is used verbatim as the call's
    /// second argument (Lua shortcuts have no separate args field; see `options_raw` on the
    /// struct), and the options table, if any, is carried through unchanged since editing never
    /// touches it.
    fn build_lua_line(&self, mods_raw: &str, key_raw: &str, dispatcher_raw: &str) -> String {
        let key_expr = lua_key_expr(mods_raw, key_raw);
        let mut line = match &self.options_raw {
            Some(options) => format!("hl.bind({key_expr}, {dispatcher_raw}, {options})"),
            None => format!("hl.bind({key_expr}, {dispatcher_raw})"),
        };
        if let Some(comment) = &self.comment {
            line.push_str(" -- ");
            line.push_str(comment);
        }
        line
    }

    /// A self-contained bind line built from this shortcut's *resolved* values rather than its
    /// raw ones, so it has no `$VAR` references. Used for template files, which are meant to be
    /// portable across configs that may not define the same variables (or any at all).
    pub fn resolved_line(&self) -> String {
        let mut fields = vec![self.mods.join(" "), self.key.clone()];
        if let Some(description) = &self.description {
            fields.push(description.clone());
        }
        fields.push(self.dispatcher.clone());
        if !self.args.is_empty() {
            fields.push(self.args.clone());
        }

        let mut line = format!("{} = {}", self.bind_type, fields.join(", "));
        if let Some(comment) = &self.comment {
            line.push_str(" # ");
            line.push_str(comment);
        }
        line
    }

    /// Whether this shortcut and `other` are bound to the same key combo (mods + key), ignoring
    /// modifier order. Used to detect conflicts when applying a template into a live config.
    pub fn same_combo(&self, other: &Shortcut) -> bool {
        self.matches_combo(&other.mods, &other.key)
    }

    /// Whether this shortcut is bound to `mods`/`key` (mods + key, ignoring modifier order).
    /// The general form `same_combo` is built on; useful when the combo to compare against
    /// isn't (yet) a `Shortcut` of its own, e.g. a candidate combo being typed into an edit
    /// field, checked before it's written anywhere.
    pub fn matches_combo(&self, mods: &[String], key: &str) -> bool {
        if self.key != key {
            return false;
        }
        let mut a = self.mods.clone();
        let mut b = mods.to_vec();
        a.sort();
        b.sort();
        a == b
    }
}

/// Build a Lua `hl.bind` key-expression string from raw mods/key text, using the `$mainMod`-style
/// marker convention (see `Shortcut::mods_raw`) to decide whether to emit a variable
/// concatenation or a plain literal string.
///
/// Only the *first* mods token is ever treated as a variable reference — the only form actually
/// used by `hl.bind` calls in practice (e.g. `mainMod .. " + SHIFT"`). Any `$` elsewhere in the
/// text is stripped rather than trusted, so a stray one can't produce invalid Lua.
fn lua_key_expr(mods_raw: &str, key_raw: &str) -> String {
    let mut tokens = mods_raw.split_whitespace();
    if let Some(var_name) = tokens.next().and_then(|first| first.strip_prefix('$')) {
        let rest: Vec<String> = tokens
            .map(|t| t.trim_start_matches('$').to_string())
            .chain(std::iter::once(key_raw.trim_start_matches('$').to_string()))
            .collect();
        return format!("{var_name} .. \" + {}\"", rest.join(" + "));
    }

    let all: Vec<String> = mods_raw
        .split_whitespace()
        .map(|t| t.trim_start_matches('$').to_string())
        .chain(std::iter::once(key_raw.trim_start_matches('$').to_string()))
        .collect();
    format!("\"{}\"", all.join(" + "))
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
            format: SourceFormat::Conf,
            options_raw: None,
        }
    }

    fn lua_shortcut() -> Shortcut {
        Shortcut {
            bind_type: "hl.bind".to_string(),
            mods: vec!["SUPER".to_string()],
            key: "RETURN".to_string(),
            description: Some("Open the terminal".to_string()),
            dispatcher: "hl.dsp.exec_cmd(\"~/.config/ml4w/settings/terminal.sh\")".to_string(),
            args: String::new(),
            comment: None,
            line: 1,
            raw: "hl.bind(mainMod .. \" + RETURN\", hl.dsp.exec_cmd(\"~/.config/ml4w/settings/terminal.sh\"), { description = \"Open the terminal\" })"
                .to_string(),
            mods_raw: "$mainMod".to_string(),
            key_raw: "RETURN".to_string(),
            description_raw: Some("Open the terminal".to_string()),
            dispatcher_raw: "hl.dsp.exec_cmd(\"~/.config/ml4w/settings/terminal.sh\")".to_string(),
            args_raw: String::new(),
            format: SourceFormat::Lua,
            options_raw: Some("{ description = \"Open the terminal\" }".to_string()),
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

    #[test]
    fn resolved_line_has_no_var_references() {
        let updated = shortcut().resolved_line();
        assert_eq!(
            updated,
            "bind = SUPER SHIFT, A, exec, ~/.config/hypr/scripts/toggle-animations.sh # Toggle animations"
        );
        assert!(!updated.contains('$'));
    }

    #[test]
    fn same_combo_ignores_modifier_order() {
        let mut other = shortcut();
        other.mods = vec!["SHIFT".to_string(), "SUPER".to_string()];
        assert!(shortcut().same_combo(&other));
    }

    #[test]
    fn same_combo_false_for_different_key_or_mods() {
        let mut different_key = shortcut();
        different_key.key = "B".to_string();
        assert!(!shortcut().same_combo(&different_key));

        let mut different_mods = shortcut();
        different_mods.mods = vec!["SUPER".to_string()];
        assert!(!shortcut().same_combo(&different_mods));
    }

    #[test]
    fn matches_combo_ignores_modifier_order() {
        assert!(shortcut().matches_combo(&["SHIFT".to_string(), "SUPER".to_string()], "A"));
    }

    #[test]
    fn matches_combo_false_for_different_key_or_mods() {
        assert!(!shortcut().matches_combo(&["SUPER".to_string(), "SHIFT".to_string()], "B"));
        assert!(!shortcut().matches_combo(&["SUPER".to_string()], "A"));
    }

    #[test]
    fn lua_with_key_rebuilds_the_variable_concatenation_and_keeps_options_and_dispatcher() {
        let updated = lua_shortcut().with_key("$mainMod SHIFT", "B");
        assert_eq!(
            updated,
            "hl.bind(mainMod .. \" + SHIFT + B\", hl.dsp.exec_cmd(\"~/.config/ml4w/settings/terminal.sh\"), { description = \"Open the terminal\" })"
        );
    }

    #[test]
    fn lua_with_key_drops_to_a_literal_string_when_the_variable_marker_is_removed() {
        let updated = lua_shortcut().with_key("SUPER SHIFT", "B");
        assert_eq!(
            updated,
            "hl.bind(\"SUPER + SHIFT + B\", hl.dsp.exec_cmd(\"~/.config/ml4w/settings/terminal.sh\"), { description = \"Open the terminal\" })"
        );
    }

    #[test]
    fn lua_with_target_replaces_only_the_dispatcher_expression() {
        let updated = lua_shortcut().with_target("hl.dsp.exec_cmd(\"~/new-script.sh\")", "");
        assert_eq!(
            updated,
            "hl.bind(mainMod .. \" + RETURN\", hl.dsp.exec_cmd(\"~/new-script.sh\"), { description = \"Open the terminal\" })"
        );
    }

    #[test]
    fn lua_with_key_omits_the_options_table_when_the_source_had_none() {
        let mut s = lua_shortcut();
        s.options_raw = None;
        let updated = s.with_key("$mainMod", "Q");
        assert_eq!(
            updated,
            "hl.bind(mainMod .. \" + Q\", hl.dsp.exec_cmd(\"~/.config/ml4w/settings/terminal.sh\"))"
        );
    }

    #[test]
    fn lua_with_key_appends_a_trailing_comment_when_present() {
        let mut s = lua_shortcut();
        s.comment = Some("Launch terminal".to_string());
        let updated = s.with_key("$mainMod", "RETURN");
        assert!(updated.ends_with(" -- Launch terminal"));
    }
}
