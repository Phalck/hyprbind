use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use super::App;
use super::paths::expand_home;

/// Common terminal emulators to look for on `$PATH`, in preference order, when nothing more
/// specific (`$TERMINAL`, a persisted setting) says which one to use.
const CANDIDATE_TERMINALS: [&str; 6] =
    ["kitty", "alacritty", "wezterm", "foot", "konsole", "xterm"];

/// Pick a default terminal command: `$TERMINAL` if it's set to something non-empty, otherwise
/// the first of `CANDIDATE_TERMINALS` found as a file in some `$PATH` entry. `None` if neither
/// turns up anything, in which case `o` tells the user to set one with `O` instead of guessing.
pub(super) fn detect_terminal() -> Option<String> {
    if let Ok(term) = std::env::var("TERMINAL")
        && !term.trim().is_empty()
    {
        return Some(term);
    }

    let path_var = std::env::var_os("PATH")?;
    CANDIDATE_TERMINALS
        .into_iter()
        .find(|candidate| std::env::split_paths(&path_var).any(|dir| dir.join(candidate).is_file()))
        .map(String::from)
}

/// If `command` looks like it runs a script — any whitespace-separated token, `~`-expanded,
/// resolves to an existing file on disk — the parent directory of the first such token. Handles
/// both a script run directly (`~/foo.sh`) and one run through an interpreter (`bash ~/foo.sh`,
/// where the first token isn't a path at all). Returns `None` for a command with no file token,
/// e.g. a pipeline of system commands like `hyprctl activewindow | grep pid | xargs kill`.
fn script_directory(command: &str) -> Option<PathBuf> {
    command
        .split_whitespace()
        .map(expand_home)
        .find(|path| path.is_file())
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Launch `command` (a program name plus any fixed arguments, e.g. "kitty --hold") with its
/// working directory set to `dir`, detached from hyprbind's own stdio so it can't write into the
/// running TUI. Fire-and-forget: never waited on, so the event loop isn't blocked while the
/// terminal stays open.
fn spawn_terminal(command: &str, dir: &Path) -> io::Result<()> {
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty terminal command"))?;
    std::process::Command::new(program)
        .args(parts)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

impl App {
    // ---- Open a terminal at a shortcut's script -------------------------------------------

    /// Open a terminal in the directory containing the script the selected shortcut's action
    /// runs, if it has one. Reports a specific status message at whichever step comes up empty,
    /// rather than doing nothing silently: the action isn't `exec`, the command doesn't point at
    /// a real script, or no terminal command is available to run.
    pub fn open_terminal_at_script(&mut self) {
        let Some(idx) = self.table_state.selected() else {
            return;
        };
        let Some(shortcut) = self.visible().get(idx).map(|s| (*s).clone()) else {
            return;
        };

        let Some(command) = shortcut.exec_command() else {
            self.set_status("Not a script: this shortcut doesn't run exec.".to_string());
            return;
        };
        let Some(dir) = script_directory(&command) else {
            self.set_status(
                "Not a script: couldn't find a script file in this command.".to_string(),
            );
            return;
        };
        let Some(terminal_command) = self.terminal_command.clone() else {
            self.set_status("No terminal command set. Press O to set one.".to_string());
            return;
        };

        match spawn_terminal(&terminal_command, &dir) {
            Ok(()) => self.set_status(format!("Opened a terminal at {}.", dir.display())),
            Err(err) => self.set_status(format!("Couldn't open a terminal: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use super::super::test_support::{edit_app, sample_shortcut};

    #[test]
    fn script_directory_resolves_a_direct_script_invocation() {
        let script = std::env::temp_dir().join(format!(
            "hyprbind-test-script-direct-{}.sh",
            std::process::id()
        ));
        fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();

        assert_eq!(
            script_directory(script.to_str().unwrap()),
            script.parent().map(Path::to_path_buf)
        );

        fs::remove_file(&script).unwrap();
    }

    #[test]
    fn script_directory_resolves_a_script_run_through_an_interpreter() {
        let script = std::env::temp_dir().join(format!(
            "hyprbind-test-script-interp-{}.sh",
            std::process::id()
        ));
        fs::write(&script, "echo hi\n").unwrap();
        let command = format!("bash {}", script.display());

        assert_eq!(
            script_directory(&command),
            script.parent().map(Path::to_path_buf)
        );

        fs::remove_file(&script).unwrap();
    }

    #[test]
    fn script_directory_is_none_without_a_file_token() {
        assert_eq!(
            script_directory("hyprctl activewindow | grep pid | xargs kill"),
            None
        );
        assert_eq!(
            script_directory("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+"),
            None
        );
    }

    #[test]
    fn open_terminal_at_script_reports_a_non_exec_shortcut() {
        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.dispatcher = "killactive".to_string();
        s.args = String::new();
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));

        app.open_terminal_at_script();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("doesn't run exec"))
        );
    }

    #[test]
    fn open_terminal_at_script_reports_no_resolvable_script() {
        let mut app = edit_app();
        app.shortcuts = vec![sample_shortcut(1, "Q")]; // dispatcher "exec", args "foo" (not a real file)
        app.table_state.select(Some(0));

        app.open_terminal_at_script();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("couldn't find a script file"))
        );
    }

    #[test]
    fn open_terminal_at_script_reports_no_terminal_command_set() {
        let script = std::env::temp_dir().join(format!(
            "hyprbind-test-open-terminal-noterm-{}.sh",
            std::process::id()
        ));
        fs::write(&script, "echo hi\n").unwrap();

        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.args = script.display().to_string();
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));
        app.terminal_command = None;

        app.open_terminal_at_script();

        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("No terminal command set"))
        );

        fs::remove_file(&script).unwrap();
    }

    #[test]
    fn open_terminal_at_script_spawns_the_terminal_command_at_the_scripts_directory() {
        let script = std::env::temp_dir().join(format!(
            "hyprbind-test-open-terminal-ok-{}.sh",
            std::process::id()
        ));
        fs::write(&script, "echo hi\n").unwrap();

        let mut app = edit_app();
        let mut s = sample_shortcut(1, "Q");
        s.args = script.display().to_string();
        app.shortcuts = vec![s];
        app.table_state.select(Some(0));
        // "true" is a real, universally-present no-op binary, so this doesn't need an actual
        // terminal emulator or a display to run headlessly in a test.
        app.terminal_command = Some("true".to_string());

        app.open_terminal_at_script();

        let expected_dir = script.parent().unwrap().display().to_string();
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s == format!("Opened a terminal at {expected_dir}.")),
            "unexpected status: {:?}",
            app.status
        );

        fs::remove_file(&script).unwrap();
    }
}
