# CachyCuts

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A terminal UI for viewing and editing Hyprland keyboard shortcuts. It targets
keybinding files managed by the ML4W (My Linux 4 Wayland) dotfiles framework.
Nothing about it is specific to CachyOS, or to any single machine: it works on
any Linux system running Hyprland with an ML4W-style dotfiles layout.

## What it does

CachyCuts parses a Hyprland keybinding file, by default:

```
~/.mydotfiles/com.ml4w.dotfiles/.config/hypr/conf/keybindings/default.conf
```

It understands the `bind` directive family used in that file (`bind`, `bindd`,
`binde`, `bindm`, `bindle`, and similar variants), resolves `$VAR` references
such as `$mainMod`, and pulls out each binding's modifiers, key, dispatcher,
arguments, and description or trailing comment. The result is shown as a
scrollable, searchable table so you can see every shortcut at a glance.

Shortcuts can be edited in place: the change is written straight back to that
same line in the source file, with everything else in the file left
untouched. CachyCuts does not yet parse the alternate Lua keybinding format
(`default.lua`).

## Requirements

- Rust (edition 2024 toolchain)
- A dotfiles checkout at `~/.mydotfiles/com.ml4w.dotfiles/` with the keybindings
  file listed above. If the file is missing, CachyCuts still starts and shows
  an error message instead of the table.

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
| `E` | Edit the `$mainMod` variable's value |
| `t` | Save shortcuts to a new template |
| `l` | Load a template |
| `T` | Set the template folder |
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

Editing is split into two scoped commands rather than one free-form line
editor:

- `e` edits the **key**: the modifiers and key, e.g. `$mainMod SHIFT, Q`.
- `a` edits the **target**: the dispatcher and its arguments, e.g.
  `exec, ~/.config/ml4w/settings/terminal.sh`.

Either opens a text field prefilled with that field exactly as written in the
source, including any `$VAR` reference — editing the key never disturbs the
target, and vice versa, so a variable like `$mainMod` in the half you're not
touching survives untouched.

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

### Editing `$mainMod`

Almost every shortcut references `$mainMod` rather than a literal modifier
key, so changing it once (e.g. from `SUPER` to `SUPER ALT`) re-points the
whole binding set. Press `E` from anywhere in the list (no row needs to be
selected) to edit its value, using the same text field and keys as above.
Saving rewrites the `$mainMod = ...` definition line; every shortcut that
references it picks up the new value on the next reload without any of their
own lines being touched. Whatever shortcut you had selected stays selected
afterward.

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

Run the test suite, which covers the keybinding parser against representative
`bind`/`bindd`/`binde` lines, variable substitution, edge cases like function
keys with no modifiers, search matching, the line-splicing logic used to
write an edit back to the file, and template save/list/append helpers:

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
  keybindings/
    model.rs             the Shortcut data type
    parser.rs             parses a Hyprland keybindings .conf file
```
