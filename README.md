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

Editing is the goal, but not implemented yet: today CachyCuts is read-only and
does not write back to the dotfiles repository. It also does not parse the
alternate Lua keybinding format (`default.lua`).

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
| `q` or Esc | Quit |

The header shows how many shortcuts were loaded and the path they came from.
If parsing fails or the file is empty, the same area shows the error instead.

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

## Development

Run the test suite, which covers the keybinding parser against representative
`bind`/`bindd`/`binde` lines, variable substitution, edge cases like function
keys with no modifiers, and search matching:

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
