use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use super::model::Shortcut;

/// Parse a Hyprland keybindings `.conf` file (e.g. `default.conf`) into a list of shortcuts.
pub fn parse_file(path: impl AsRef<Path>) -> io::Result<Vec<Shortcut>> {
    let contents = fs::read_to_string(path)?;
    Ok(parse_str(&contents))
}

/// Parse the contents of a Hyprland keybindings `.conf` file.
///
/// Handles `$VAR = value` substitution and the `bind`/`bindd`/`binde`/`bindm`/`bindle`/...
/// directive family. Full-line comments and blank lines are skipped. Does not understand the
/// Lua keybinding format (`default.lua`).
pub fn parse_str(contents: &str) -> Vec<Shortcut> {
    let mut vars: HashMap<String, String> = HashMap::new();
    let mut shortcuts = Vec::new();

    for (idx, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        let line_no = idx + 1;

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix('$') {
            if let Some((name, value)) = rest.split_once('=') {
                vars.insert(format!("${}", name.trim()), value.trim().to_string());
            }
            continue;
        }

        if let Some(shortcut) = parse_bind_line(line, raw_line, line_no, &vars) {
            shortcuts.push(shortcut);
        }
    }

    shortcuts
}

fn parse_bind_line(
    line: &str,
    raw_line: &str,
    line_no: usize,
    vars: &HashMap<String, String>,
) -> Option<Shortcut> {
    if !line.starts_with("bind") {
        return None;
    }
    let (directive, rest) = line.split_once('=')?;
    let bind_type = directive.trim().to_string();
    if !bind_type.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }

    let (body, comment) = match rest.split_once('#') {
        Some((body, comment)) => (body, Some(comment.trim().to_string())),
        None => (rest, None),
    };

    let field_count = if bind_type == "bindd" { 5 } else { 4 };
    let fields: Vec<&str> = body.splitn(field_count, ',').map(str::trim).collect();

    let mods_raw = fields.first().copied().unwrap_or("").to_string();
    let key_raw = fields.get(1).copied().unwrap_or("").to_string();

    let mods = mods_raw
        .split_whitespace()
        .map(|tok| substitute(tok, vars))
        .collect();
    let key = substitute(&key_raw, vars);

    let (description_raw, dispatcher_raw, args_raw) = if bind_type == "bindd" {
        (
            fields.get(2).map(|d| d.to_string()),
            fields.get(3).copied().unwrap_or("").to_string(),
            fields.get(4).copied().unwrap_or("").to_string(),
        )
    } else {
        (
            None,
            fields.get(2).copied().unwrap_or("").to_string(),
            fields.get(3).copied().unwrap_or("").to_string(),
        )
    };
    let description = description_raw.as_deref().map(|d| substitute(d, vars));
    let dispatcher = substitute(&dispatcher_raw, vars);
    let args = substitute(&args_raw, vars);

    if key.is_empty() && dispatcher.is_empty() {
        return None;
    }

    Some(Shortcut {
        bind_type,
        mods,
        key,
        description,
        dispatcher,
        args,
        comment,
        line: line_no,
        raw: raw_line.to_string(),
        mods_raw,
        key_raw,
        description_raw,
        dispatcher_raw,
        args_raw,
    })
}

/// Replace `$VAR` references with their defined value. Leaves unknown variables untouched.
fn substitute(text: &str, vars: &HashMap<String, String>) -> String {
    let mut result = text.to_string();
    for (name, value) in vars {
        result = result.replace(name, value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_bind() {
        let input = "$mainMod = SUPER\nbind = $mainMod, Q, killactive # Kill active window\n";
        let shortcuts = parse_str(input);
        assert_eq!(shortcuts.len(), 1);
        let s = &shortcuts[0];
        assert_eq!(s.bind_type, "bind");
        assert_eq!(s.mods, vec!["SUPER"]);
        assert_eq!(s.key, "Q");
        assert_eq!(s.dispatcher, "killactive");
        assert_eq!(s.args, "");
        assert_eq!(s.comment.as_deref(), Some("Kill active window"));
    }

    #[test]
    fn parses_exec_with_args_and_resolves_vars() {
        let input =
            "$mainMod = SUPER\n$HYPRSCRIPTS = ~/.config/hypr/scripts\nbind = $mainMod SHIFT, A, exec, $HYPRSCRIPTS/toggle-animations.sh # Toggle animations\n";
        let shortcuts = parse_str(input);
        assert_eq!(shortcuts.len(), 1);
        let s = &shortcuts[0];
        assert_eq!(s.mods, vec!["SUPER", "SHIFT"]);
        assert_eq!(s.dispatcher, "exec");
        assert_eq!(s.args, "~/.config/hypr/scripts/toggle-animations.sh");
    }

    #[test]
    fn parses_bindd_with_description() {
        let input = "$mainMod = SUPER\nbindd = $mainMod SHIFT, T, Float all windows, exec, ~/.config/ml4w/scripts/ml4w-toggle-allfloat\n";
        let shortcuts = parse_str(input);
        assert_eq!(shortcuts.len(), 1);
        let s = &shortcuts[0];
        assert_eq!(s.bind_type, "bindd");
        assert_eq!(s.description.as_deref(), Some("Float all windows"));
        assert_eq!(s.dispatcher, "exec");
    }

    #[test]
    fn parses_fn_key_with_no_mods() {
        let input = "bind = , XF86AudioMute, exec, pactl set-sink-mute @DEFAULT_SINK@ toggle # Toggle mute\n";
        let shortcuts = parse_str(input);
        assert_eq!(shortcuts.len(), 1);
        assert!(shortcuts[0].mods.is_empty());
        assert_eq!(shortcuts[0].key, "XF86AudioMute");
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let input = "# just a comment\n\n   \n# another\n";
        assert!(parse_str(input).is_empty());
    }
}
