# hyprbind

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A terminal UI for viewing and editing Hyprland keyboard shortcuts. It works
directly with Hyprland's own `bind` config syntax and with ML4W's Lua
keybinding DSL, not a distribution-specific format, so parsing and editing
shortcuts works on any Hyprland setup, on any distribution.

## What it does

hyprbind parses a Hyprland keybinding file. By default it tries the path
used by the ML4W (My Linux 4 Wayland) dotfiles framework:

```
~/.mydotfiles/com.ml4w.dotfiles/.config/hypr/conf/keybindings/default.conf
```

If that's not there, it searches `~/.config/hypr` — the one location every
Hyprland install has, regardless of distribution or dotfiles framework — for
whichever `.conf` or `.lua` file has the most recognizable shortcuts in it,
and uses that instead. See
[Keybindings file location](#keybindings-file-location) below for how this
works and how to point it somewhere else entirely.

Two source syntaxes are understood:

- **`.conf`** — the `bind` directive family (`bind`, `bindd`, `binde`,
  `bindm`, `bindle`, and similar variants), with `$VAR` references such as
  `$mainMod` resolved.
- **Lua** (`default.lua`) — ML4W's `hl.bind(key_expr, dispatcher_expr, {
  options })` calls, with a local variable used in the key expression (e.g.
  `mainMod .. " + SHIFT + Q"`) resolved the same way `$mainMod` is for
  `.conf`. Only single-line `hl.bind` calls are parsed; anything generated
  inside a `for` loop (e.g. ML4W's default per-workspace bindings) is skipped,
  since there's no single static line to edit or write back to.

Either way, hyprbind pulls out each binding's modifiers, key, dispatcher,
arguments, and description or trailing comment. The result is shown as a
scrollable, searchable table so you can see every shortcut at a glance.

Shortcuts can be edited in place: the change is written straight back to
that same line in the source file — in whichever syntax it was written in —
with everything else in the file left untouched. Saving and applying
templates (`t`/`l`, see [Templates](#templates)) is `.conf`-syntax only for
now: there's no reliable way to translate an arbitrary Lua `hl.dsp....`
dispatcher call into a Hyprland `.conf` dispatcher, or back, so hyprbind
refuses rather than risk writing something broken.

## Requirements

- Rust (edition 2024 toolchain)
- A Hyprland keybindings file hyprbind can find or be told about — see
  [Keybindings file location](#keybindings-file-location). If none can be
  found, hyprbind still starts and shows an error message instead of the
  table.

## Install and run

```sh
cargo build
cargo run
```

## Usage

| Key | Action |
| --- | --- |
| `j` or Down | Move selection down |
| `k` or Up | Move selection up |
| `g` | Jump to the first shortcut |
| `G` | Jump to the last shortcut |
| `/` | Start or edit a search |
| `e` | Edit the selected shortcut's key |
| `a` | Edit the selected shortcut's target (dispatcher and arguments) |
| `d` | Edit the selected shortcut's description |
| `A` | Add a new shortcut |
| `E` | Edit the `$mainMod` variable's value |
| `o` | Open a terminal in the directory of the script the selected shortcut runs |
| `t` | Save shortcuts to a new template |
| `l` | Load a template |
| `T` | Set the template folder |
| `S` | Set the keybindings file path |
| `b` | Back up the current keybindings file |
| `r` | Restore from a backup |
| `B` | Set the backup folder |
| `O` | Set the terminal command used by `o` |
| `q` or Esc | Quit |

The header shows how many shortcuts were loaded and the path they came from,
plus a second line naming whichever command is currently active (`Browse`,
`Search`, `Edit key`, `Save template`, and so on) with a one-line reminder of
what it does and how to use it — it updates as you move between commands.
If parsing fails or the file is empty, the first line shows the error instead.

Actions that report a result (saving an edit, saving or applying a template,
setting the template folder) show it in the footer in place of the key hints.
That message clears itself after 5 seconds, even if you don't press anything,
so the hints come back on their own.

The key hints themselves are split into two groups: everyday commands, and a
`Settings:`-labeled group for the file-path/folder/command commands (`E`,
`T`, `S`, `B`, `O`) — kept visually separate so they don't crowd out the
commands you use constantly. Each group word-wraps independently to fit the
terminal's actual width, so every command stays visible (on more lines, if
the terminal is narrow) rather than being cut off.

### Search and filtering

Press `/` to open the search box. The table filters live as you type, matching
against the key combo, dispatcher and arguments, and description or comment of
each shortcut, case-insensitively.

| Key | Action |
| --- | --- |
| Any character | Add to the search query |
| Backspace | Remove the last character |
| Up / Down | Move selection within the filtered results |
| Enter | Apply the filter and return to normal mode |
| Esc | Cancel the search and clear the filter |

While a filter is active, press `/` again to edit it, or clear it by
backspacing to an empty query and pressing Enter (or by pressing Esc).

### Editing

Editing is split into scoped commands rather than one free-form line editor:

- `e` edits the **key**: the modifiers and key, e.g. `$mainMod SHIFT, Q`.
- `a` edits the **target**: the dispatcher and its arguments, e.g.
  `exec, ~/.config/ml4w/settings/terminal.sh`.
- `d` edits the **description** — see
  [Editing the description](#editing-the-description) below, since it works a
  little differently.

Each opens a text field prefilled with that field exactly as written in the
source, including any `$VAR` reference — editing one never disturbs the
others, so a variable like `$mainMod` in a field you're not touching survives
untouched.

| Key | Action |
| --- | --- |
| Any character | Insert at the cursor |
| Left / Right | Move the cursor one character |
| Home / End | Jump to the start / end of the field |
| Backspace | Delete the character before the cursor |
| Delete | Delete the character at the cursor |
| Enter | Save: write the change back to the file, in place |
| Esc | Cancel without touching the file |

For the key field, text before the first comma is the modifiers and text
after is the key (no comma means no modifiers, just a key). For the target
field, text before the first comma is the dispatcher and the rest is its
arguments (no comma means a dispatcher with no arguments, e.g. `killactive`).
The description field (`d`) isn't split at all — the whole field is the
description text, comma and all.

Saving rebuilds the whole source line from its fields (mods, key,
description, dispatcher, arguments, comment) and replaces only that one line
in the file; every other line is left byte-for-byte as it was. Field content
is always preserved exactly, but separators are normalized to a consistent
`field, field, ... # comment` style, so any original column-alignment padding
around commas or before a comment is lost on save. The write is atomic
(written to a temp file, then renamed into place), so a failure partway
through can't leave the keybindings file half-written. After a successful
save, the table reloads from disk and the row you edited stays selected.

Because this edits the file Hyprland reads its keybindings from, keep that
dotfiles path under version control (as ML4W setups normally are) so you can
diff or revert a change you don't want.

**Duplicate check.** Pressing Enter after `e` doesn't write immediately if
the new combo (mods + key, `$VAR` references resolved, modifier order
ignored) is already used by another shortcut. Instead you get a confirmation
screen naming the conflicting shortcut, plus — if one of `SUPER`, `SHIFT`,
`CTRL`, or `ALT` isn't already part of the combo and adding it wouldn't
itself collide with anything — a suggested fix, e.g. "already used by ...;
Enter to use SUPER + ALT + Q instead and save." Enter there applies exactly
that suggestion (nothing else changes); Esc cancels the whole edit, leaving
the original key untouched. If no unused modifier resolves the conflict, only
Esc is offered. This check only applies to `e` (the key); editing the target
with `a` can't create a duplicate, since the key doesn't change.

### Editing the description

`d` opens the same text field as `e`/`a`, prefilled with whatever the table's
Description column is already showing for that row — which is usually a
trailing comment, not a formal description field; see below. Empty input is
rejected — an empty text box just means "nothing set," not "clear it" — so
save something or press Esc to cancel.

Which underlying field actually gets changed depends on what the shortcut
already has, in the same order the Description column itself prefers:

- If it already has an explicit description — `.conf`'s `bindd` field, or
  Lua's `description = "..."` entry — that's what gets replaced.
- Otherwise, its trailing comment (`# ...` for `.conf`, `-- ...` for Lua) is
  added or replaced instead. This is the common case: most real-world
  `.conf` keybindings (including ML4W's own defaults) use a plain `bind`
  with a trailing comment rather than `bindd`, so `d` edits that, not a
  `bindd` field that doesn't exist. A plain `bind` is never upgraded to
  `bindd` just to gain a description field — a comment does the same job
  and every directive (`bind`, `binde`, `bindm`, `bindle`, ...) supports one
  equally, with no guessing required.
- For a Lua bind with neither an options table nor a comment yet, a fresh
  options table is created instead (`{ description = "..." }`), since that's
  where a real Lua bind's description almost always lives.

Either way, everything else about the line — mods, key, dispatcher,
arguments, any other option in a Lua table — is left exactly as it was.

### Adding a shortcut

`A` starts a new shortcut, `.conf`-syntax only (see the note on templates
above — the same reasoning applies: there's no reliable way to guess a valid
Lua `hl.bind` dispatcher call from nothing). It opens the same text field as
`e`/`a`/`d`, but for the **description**, since that's the one field with no
existing binding to prefill from. Empty input is rejected, same as `d`.

Enter appends a new line —

```
bind = , CHANGEME, exec # <your description>
```

— to the end of the keybindings file, selects it, and shows a reminder in the
footer. `CHANGEME` isn't a real Hyprland key, so nothing is actually bound
yet; press `e` to give it a real key combo and `a` to set its dispatcher and
arguments, the same way you would for any other shortcut. Until you do, the
placeholder line just sits there ignored by Hyprland.

### Editing `$mainMod`

Almost every shortcut references `$mainMod` rather than a literal modifier
key, so changing it once (e.g. from `SUPER` to `SUPER ALT`) re-points the
whole binding set. Press `E` from anywhere in the list (no row needs to be
selected) to edit its value, using the same text field and keys as above.
Saving rewrites the `$mainMod = ...` definition line; every shortcut that
references it picks up the new value on the next reload without any of their
own lines being touched. Whatever shortcut you had selected stays selected
afterward.

### Opening a shortcut's script

Many shortcuts don't run a command directly — they `exec` a small dispatcher
script (ML4W's `~/.config/ml4w/settings/*.sh` scripts are a good example: a
one-line file just naming the actual browser, terminal, etc. to run). `o`
opens a terminal in the directory containing the script the *selected*
shortcut runs, so you can read or edit it without leaving hyprbind to go find
it by hand.

`o` only does something when there's a script to point at:

- The shortcut's action has to be an `exec` (`.conf`'s `exec`/`execr`, or
  Lua's `hl.dsp.exec_cmd(...)`) — not a window/workspace-management dispatcher
  or anything else, since those don't run a script at all.
- At least one whitespace-separated token of the command, after `~`
  expansion, has to resolve to a real file on disk. This isn't limited to
  `.sh` files, and it isn't limited to the first token either, so both a
  script run directly (`~/foo.sh`) and one run through an interpreter (`bash
  ~/foo.sh`) are found. A system command with no script in it at all (`hyprctl
  reload`, `wpctl set-volume ...`, a shell pipeline) has no such token, so `o`
  reports that instead of guessing.

Either way you get a status message explaining what happened — which
dispatcher it needed, or which directory it opened.

The terminal itself is whatever `terminal_command` is set to: on first run
this is auto-detected (`$TERMINAL` if it's set, otherwise the first common
terminal emulator — kitty, alacritty, wezterm, foot, konsole, xterm — found
on `$PATH`), and can be overridden any time with `O` (same text field as the
other settings, e.g. `alacritty` or `kitty --hold`). It's launched with its
working directory set to the script's folder — not by passing it a
directory flag, so this works the same regardless of which terminal it is —
detached from hyprbind's own input/output so it can't interfere with the
running TUI. If nothing was detected and nothing was set, `o` says so and
points you at `O`.

### Keybindings file location

At startup, hyprbind picks a keybindings file to use in three steps:

1. Use whatever path was persisted from a previous run (see
   [Persisted settings](#persisted-settings) below), if any.
2. Otherwise, try the ML4W default path (see [What it does](#what-it-does)).
3. If that's missing or has no shortcuts in it either, search
   `~/.config/hypr` recursively for `.conf` and `.lua` files, parse each
   one, and switch to whichever has the most recognizable shortcuts. This
   follows
   symlinked directories (so dotfiles managers that populate
   `~/.config/hypr` via symlinks, including ML4W's own, still resolve
   correctly) but is cycle-safe. If this finds a file, the status line
   announces it — `Auto-detected keybindings file: ...` — and that path is
   persisted too, on the reasoning that whatever step 1 or 2 pointed at is
   now known to be broken, so there's nothing worth keeping around.

If nothing is found at all, hyprbind starts with an empty list and an error
naming the path it tried, pointing you at `S`.

Press `S` at any time to set the path directly — it opens the same text
field as the other settings above, prefilled with the current path (`~` is
expanded). Unlike the template folder, a bad value here is never silently
accepted: hyprbind tries to parse whatever you enter, and only switches over
if that file exists and has at least one shortcut in it. If it doesn't
(typo, wrong path, empty file), you get an error and the file you were
already using stays active — you're never left looking at a blank list with
no way back.

### Persisted settings

The keybindings file path, the template folder, the backup folder, and the
terminal command all persist across runs, in `~/.config/hyprbind/config` — a
plain `key = value` text file, not a structured format like TOML, since
there are only a handful of settings to store. Every time one is changed
successfully (`S`, `T`, `B`, or `O`), it's written there immediately, and the
status line's confirmation gets a `(saved)` suffix to confirm it. There's no
separate command to edit this file's own location or turn persistence off;
delete it (or a line from it) to reset that setting back to the automatic
behavior described above.

### Backups

A backup is a `.hbb` ("hyprbind backup") file: an exact byte-for-byte copy of
the current keybindings file, made with `b`. Unlike templates, nothing is
parsed or resolved — every comment, `$VAR` definition, and bit of formatting
is preserved, because the point of a backup is to be able to put the file
back exactly as it was. Backups are named `<original filename>-<timestamp>.hbb`
and stored in a folder that defaults to `$HOME`; change it with `B` (same
text field as the other settings above, `~` expanded).

Restoring (`r`) is the one destructive action in hyprbind — it overwrites the
whole keybindings file — so it's the only command with an extra confirmation
step instead of committing immediately:

1. `r` lists every `.hbb` file in the backup folder.
2. Pick one and press Enter to see a confirmation screen naming the backup
   and the file it would overwrite.
3. Enter there restores it (atomically, so a failure partway through can't
   leave the file half-written) and reloads the table; Esc at either step
   cancels without changing anything.

### Templates

A template is a `.hbt` ("hyperbind template") file: a small text file holding
a chosen subset of shortcuts, written using their *resolved* values (no
`$VAR` references), so it's portable to a config that doesn't define the same
variables, or any at all. Templates are stored in a folder that defaults to
`$HOME`; change it with `T` (opens the same text field as the editing
commands above, prefilled with the current folder, `~` is expanded).

**Saving (`t`):** opens a checkbox list of the shortcuts currently in view
(respecting an active search filter, so you can narrow the list down first).

| Key | Action |
| --- | --- |
| Up / Down or `j` / `k` | Move the cursor |
| Space | Toggle the shortcut under the cursor |
| Enter | Continue to naming the file (at least one must be checked) |
| Esc | Cancel, discarding the selection |

After checking the shortcuts you want, Enter moves to a text field for the
template's name (no path separators allowed); Enter there writes
`<template folder>/<name>.hbt`, Esc abandons the whole save without writing
anything.

**Loading (`l`):** lists every `.hbt` file in the template folder. Pick one
and press Enter to see the shortcuts it contains, in the same checkbox list
as saving — everything starts checked, so pressing Enter immediately applies
the whole template, or uncheck anything you don't want first.

| Key | Action |
| --- | --- |
| Up / Down or `j` / `k` | Move the cursor |
| Space | Toggle the shortcut under the cursor |
| Enter | Apply the checked shortcuts |
| Esc | Cancel without changing anything |

Applying appends the checked shortcuts to the end of the keybindings file,
after a blank line and a `# Applied from template: <name>.hbt` marker
comment, so they're easy to find afterward; every existing line is left
untouched. Before appending, each checked shortcut is checked against your
current shortcuts by key combo (mods + key, regardless of modifier order) —
anything already bound is skipped rather than creating a conflicting
duplicate bind, and the status line reports how many were applied versus
skipped.

## Development

Run the test suite, which covers the `.conf` keybinding parser against
representative `bind`/`bindd`/`binde` lines, the Lua parser against
representative `hl.bind` calls (literal and variable-concatenated key
expressions, nested dispatcher-table arguments, `for`-loop skipping, and
`local` variable capture), variable substitution in both formats, edge cases
like function keys with no modifiers, search matching, the line-splicing
logic used to write an edit back to the file in either syntax, template
save/list/append helpers (and the guard that refuses to save or apply a
template against a Lua source), the keybindings-file auto-detection scan
(including symlink-cycle safety), the persisted-settings file (parsing,
round-tripping, and the app-level integration that writes it on a successful
`S`/`T`/`B`/`O`), backup/restore (byte-for-byte copy on backup, atomic
overwrite plus reload on restore, and failure handling for both),
duplicate-key detection (no self-conflict on an unchanged combo, variable
resolution when comparing, the fix search finding or failing to find an
unused modifier, and both outcomes of the confirm screen), `o`'s
script-detection logic (recognizing `exec`-style dispatchers in both
formats, resolving a script run directly vs. through an interpreter,
correctly finding nothing in a system-command pipeline, and the terminal
actually getting spawned with the right working directory), and `d`'s
description editing (falling back to the trailing comment when there's no
`bindd`/`description` field, same as the table display, for any `.conf`
directive; replacing an existing `bindd` description without touching its
comment; and the Lua cases — replacing an existing `description` entry,
appending one to a table that has other options but not that one, creating a
fresh options table from scratch, and preferring an existing trailing
comment over creating one), and the footer's word-wrapping (items packed
onto as few lines as fit a given width, none exceeding it, an
oversized single item still getting its own line rather than being dropped,
and a label's width counting toward the first line):

```sh
cargo test
```

## Stack

- Rust
- [ratatui](https://ratatui.rs) for the terminal UI
- [crossterm](https://github.com/crossterm-rs/crossterm) for terminal input and control

## Project layout

```
src/
  main.rs               entry point and the key handling loop
  app.rs                application state: loaded shortcuts, selection, errors
  ui.rs                 rendering: title bar, table, footer
  config.rs             reads/writes ~/.config/hyprbind/config
  fs_util.rs            shared atomic-write helper
  keybindings/
    model.rs             the Shortcut data type, shared by both source formats
    parser.rs             parses a Hyprland keybindings .conf file
    lua_parser.rs          parses ML4W's Lua keybinding format (default.lua)
    discover.rs           searches ~/.config/hypr for the keybindings file
```
