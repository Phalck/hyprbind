# CachyCuts

A terminal UI for browsing keyboard shortcuts on this machine. The machine runs
Hyprland, configured through the ML4W (My Linux 4 Wayland) dotfiles framework,
and CachyCuts reads the keybindings straight out of that dotfiles repository.

## What it does

CachyCuts parses the active Hyprland keybinding file:

```
~/.mydotfiles/com.ml4w.dotfiles/.config/hypr/conf/keybindings/default.conf
```

It understands the `bind` directive family used in that file (`bind`, `bindd`,
`binde`, `bindm`, `bindle`, and similar variants), resolves `$VAR` references
such as `$mainMod`, and pulls out each binding's modifiers, key, dispatcher,
arguments, and description or trailing comment. The result is shown as a
scrollable table so you can see every shortcut on the system at a glance.

This is a read-only browser for now. It does not edit or write back to the
dotfiles repository, and it does not parse the alternate Lua keybinding format
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
| `q` or Esc | Quit |

The header shows how many shortcuts were loaded and the path they came from.
If parsing fails or the file is empty, the same area shows the error instead.

## Development

Run the test suite, which covers the keybinding parser against representative
`bind`/`bindd`/`binde` lines, variable substitution, and edge cases like
function keys with no modifiers:

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
