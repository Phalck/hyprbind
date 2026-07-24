use std::collections::HashMap;

use super::model::{ParsedConfig, Shortcut, SourceFormat, Variable};

/// Parse the contents of ML4W's Lua keybinding format (`default.lua`).
///
/// Understands top-level `local NAME = "value"` variable definitions and single-line
/// `hl.bind(key_expr, dispatcher_expr, { options })` calls, where `key_expr` is either a plain
/// literal string (`"CTRL + ALT + T"`) or a single variable concatenated with one
/// (`mainMod .. " + SHIFT + Q"`) — the only two forms `hl.bind` is actually called with in
/// practice. Anything else (multi-line calls, more than one variable in the key expression, a
/// non-literal dispatcher table) is silently skipped rather than guessed at, since hyprbind can
/// only safely edit and write back what it's confident it parsed correctly.
///
/// Binds inside a `for ... do ... end` loop are skipped entirely: their key expressions and
/// dispatcher arguments depend on the loop variable, so there's no single static line to edit or
/// write back to.
pub fn parse_str(contents: &str) -> ParsedConfig {
    let mut vars: HashMap<String, String> = HashMap::new();
    let mut variables = Vec::new();
    let mut shortcuts = Vec::new();
    let mut loop_depth: u32 = 0;

    for (idx, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        let line_no = idx + 1;

        if line.is_empty() || line.starts_with("--") {
            continue;
        }

        if loop_depth > 0 {
            if line == "end" || line.starts_with("end ") {
                loop_depth -= 1;
            } else if is_for_loop_start(line) {
                loop_depth += 1;
            }
            continue;
        }

        if is_for_loop_start(line) {
            loop_depth += 1;
            continue;
        }

        if let Some((name, value, comment)) = parse_local_string(line) {
            vars.insert(name.clone(), value.clone());
            variables.push(Variable {
                name,
                value,
                line: line_no,
                format: SourceFormat::Lua,
                comment,
                raw: raw_line.to_string(),
            });
            continue;
        }

        if let Some(shortcut) = parse_bind_line(line, raw_line, line_no, &vars) {
            shortcuts.push(shortcut);
        }
    }

    ParsedConfig { shortcuts, variables }
}

fn is_for_loop_start(line: &str) -> bool {
    line.starts_with("for ") && line.ends_with(" do")
}

/// Match `local NAME = "value"`, optionally followed by a `-- comment`. Deliberately requires a
/// quoted string value, so a non-string assignment like `local key = i % 10` (seen inside the
/// workspace-binding loop) is never mistaken for a hyprbind-editable variable.
fn parse_local_string(line: &str) -> Option<(String, String, Option<String>)> {
    let rest = line.strip_prefix("local ")?;
    let (name, rest) = rest.split_once('=')?;
    let name = name.trim().to_string();
    if !is_ident(&name) {
        return None;
    }

    let rest = rest.trim().strip_prefix('"')?;
    let (value, after) = rest.split_once('"')?;
    let comment = after.trim().strip_prefix("--").map(|c| c.trim().to_string());
    Some((name, value.to_string(), comment))
}

fn parse_bind_line(
    line: &str,
    raw_line: &str,
    line_no: usize,
    vars: &HashMap<String, String>,
) -> Option<Shortcut> {
    let after_open = line.strip_prefix("hl.bind(")?;
    let (args_blob, after_call) = split_call_args(after_open)?;

    let parts = split_top_level_commas(args_blob);
    if parts.len() < 2 {
        return None;
    }
    let key_expr = parts[0].trim();
    let dispatcher_raw = parts[1].trim().to_string();
    if dispatcher_raw.is_empty() {
        return None;
    }
    let options_raw = parts.get(2).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    let (mods_raw, key_raw, mods, key) = parse_key_expr(key_expr, vars)?;
    let description = options_raw.as_deref().and_then(extract_description);
    let comment = extract_trailing_comment(after_call);

    Some(Shortcut {
        bind_type: "hl.bind".to_string(),
        mods,
        key,
        description: description.clone(),
        dispatcher: dispatcher_raw.clone(),
        args: String::new(),
        comment,
        line: line_no,
        raw: raw_line.to_string(),
        mods_raw,
        key_raw,
        description_raw: description,
        dispatcher_raw,
        args_raw: String::new(),
        format: SourceFormat::Lua,
        options_raw,
    })
}

/// Split a Hyprland-modifier key expression (`hl.bind`'s first argument) into raw (unresolved,
/// `$VAR`-marked, see `Shortcut::mods_raw`) and resolved mods/key. Returns `(mods_raw, key_raw,
/// mods, key)`.
fn parse_key_expr(expr: &str, vars: &HashMap<String, String>) -> Option<(String, String, Vec<String>, String)> {
    let (raw_tokens, resolved_tokens): (Vec<String>, Vec<String>) = if let Some((var_part, str_part)) =
        expr.split_once("..")
    {
        let var_name = var_part.trim();
        if !is_ident(var_name) {
            return None;
        }
        let literal = extract_quoted(str_part.trim())?;
        let str_tokens: Vec<&str> = literal.split(" + ").map(str::trim).filter(|t| !t.is_empty()).collect();
        if str_tokens.is_empty() {
            return None;
        }

        let mut raw = vec![format!("${var_name}")];
        raw.extend(str_tokens.iter().map(|t| t.to_string()));

        let resolved_var = vars.get(var_name).cloned().unwrap_or_else(|| var_name.to_string());
        let mut resolved = vec![resolved_var];
        resolved.extend(str_tokens.iter().map(|t| t.to_string()));

        (raw, resolved)
    } else {
        let literal = extract_quoted(expr.trim())?;
        let tokens: Vec<String> =
            literal.split(" + ").map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect();
        if tokens.is_empty() {
            return None;
        }
        (tokens.clone(), tokens)
    };

    let key_raw = raw_tokens.last()?.clone();
    let mods_raw = raw_tokens[..raw_tokens.len() - 1].join(" ");
    let key = resolved_tokens.last()?.clone();
    let mods = resolved_tokens[..resolved_tokens.len() - 1].to_vec();
    Some((mods_raw, key_raw, mods, key))
}

/// Find the matching `)` that closes a call whose opening `(` has already been consumed, tracking
/// nested `()`/`{}`/`[]` (bracket *kinds* aren't matched against each other, just counted as one
/// combined depth, which is enough to find the right close for well-formed Lua) and skipping
/// anything inside a `"..."` string. Returns the text between the parens and everything after the
/// close.
fn split_call_args(after_open_paren: &str) -> Option<(&str, &str)> {
    let mut depth = 1i32;
    let mut in_string = false;
    for (i, c) in after_open_paren.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '(' | '{' | '[' if !in_string => depth += 1,
            ')' | '}' | ']' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some((&after_open_paren[..i], &after_open_paren[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Split `s` on commas that appear outside any bracket nesting or string, so a dispatcher call's
/// own arguments (e.g. `hl.dsp.window.resize({ x = 100, y = 0 })`) aren't mistaken for separate
/// `hl.bind` arguments.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '(' | '{' | '[' if !in_string => depth += 1,
            ')' | '}' | ']' if !in_string => depth -= 1,
            ',' if !in_string && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

fn extract_quoted(s: &str) -> Option<String> {
    Some(s.strip_prefix('"')?.strip_suffix('"')?.to_string())
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Pull `description = "..."` out of an `hl.bind` options table's raw text, if present.
fn extract_description(options: &str) -> Option<String> {
    let idx = options.find("description")?;
    let after = options[idx + "description".len()..].trim_start().strip_prefix('=')?.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// A `-- comment` trailing the closing `)` of an `hl.bind(...)` call, on the same line.
fn extract_trailing_comment(after_call: &str) -> Option<String> {
    after_call.trim_start().strip_prefix("--").map(|c| c.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_literal_key_bind_with_description() {
        let input = "hl.bind(\"CTRL + ALT + T\", hl.dsp.exec_cmd(\"~/.config/ml4w/themes/themes.sh\"), { description = \"Open Select Window Menu\" })\n";
        let shortcuts = parse_str(input).shortcuts;
        assert_eq!(shortcuts.len(), 1);
        let s = &shortcuts[0];
        assert_eq!(s.mods, vec!["CTRL", "ALT"]);
        assert_eq!(s.key, "T");
        assert_eq!(s.dispatcher, "hl.dsp.exec_cmd(\"~/.config/ml4w/themes/themes.sh\")");
        assert_eq!(s.args, "");
        assert_eq!(s.description.as_deref(), Some("Open Select Window Menu"));
        assert_eq!(s.format, SourceFormat::Lua);
    }

    #[test]
    fn parses_a_variable_concatenated_key_and_resolves_it() {
        let input = "local mainMod = \"SUPER\"\nhl.bind(mainMod .. \" + SHIFT + Q\", hl.dsp.exec_cmd(\"kill.sh\"), { description = \"Kill\" })\n";
        let shortcuts = parse_str(input).shortcuts;
        assert_eq!(shortcuts.len(), 1);
        let s = &shortcuts[0];
        assert_eq!(s.mods, vec!["SUPER", "SHIFT"]);
        assert_eq!(s.key, "Q");
        assert_eq!(s.mods_raw, "$mainMod SHIFT");
        assert_eq!(s.key_raw, "Q");
    }

    #[test]
    fn preserves_a_nested_dispatcher_table_and_finds_the_top_level_comma() {
        let input = "hl.bind(mainMod .. \" + SHIFT + right\", hl.dsp.window.resize({ x = 100, y = 0 }), { description = \"Widen\" })\n";
        let shortcuts = parse_str(input).shortcuts;
        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0].dispatcher, "hl.dsp.window.resize({ x = 100, y = 0 })");
    }

    #[test]
    fn skips_binds_inside_a_for_loop() {
        let input = "for i = 1, 10 do\n    local key = i % 10\n    hl.bind(mainMod .. \" + \" .. key, hl.dsp.focus({ workspace = i }), { description = \"Focus\" })\nend\nhl.bind(\"CTRL + ALT + T\", hl.dsp.exec_cmd(\"x\"), { description = \"y\" })\n";
        let shortcuts = parse_str(input).shortcuts;
        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0].key, "T");
    }

    #[test]
    fn skips_full_line_comments_including_commented_out_binds() {
        let input = "-- hl.bind(mainMod .. \" + Q\", hl.dsp.window.close(), { description = \"Kill\" })\n";
        assert!(parse_str(input).shortcuts.is_empty());
    }

    #[test]
    fn captures_local_string_variables_with_trailing_comments() {
        let input = "local mainMod = \"SUPER\" -- Sets \"Windows\" key as main modifier\n";
        let variables = parse_str(input).variables;
        assert_eq!(variables.len(), 1);
        assert_eq!(variables[0].name, "mainMod");
        assert_eq!(variables[0].value, "SUPER");
        assert_eq!(variables[0].comment.as_deref(), Some("Sets \"Windows\" key as main modifier"));
        assert_eq!(variables[0].format, SourceFormat::Lua);
    }

    #[test]
    fn does_not_capture_a_non_string_local_assignment() {
        let input = "local key = i % 10 -- 10 maps to key 0\n";
        assert!(parse_str(input).variables.is_empty());
    }

    #[test]
    fn a_bind_with_no_options_table_has_no_description() {
        let input = "hl.bind(\"CTRL + ALT + T\", hl.dsp.exec_cmd(\"x\"))\n";
        let shortcuts = parse_str(input).shortcuts;
        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0].description, None);
        assert_eq!(shortcuts[0].options_raw, None);
    }

    #[test]
    fn captures_a_trailing_line_comment_on_a_bind_call() {
        let input = "hl.bind(\"CTRL + ALT + T\", hl.dsp.exec_cmd(\"x\"), { description = \"y\" }) -- legacy binding\n";
        let shortcuts = parse_str(input).shortcuts;
        assert_eq!(shortcuts[0].comment.as_deref(), Some("legacy binding"));
    }
}
