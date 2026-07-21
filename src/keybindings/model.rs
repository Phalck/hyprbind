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
    /// 1-based line number in the source file, kept for future editing/write-back.
    pub line: usize,
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
}
