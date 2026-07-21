# CLAUDE.md

Guidance for Claude Code when working in this repo.

## What this is

CachyCuts: a TUI (Rust, ratatui + crossterm) for viewing and editing keyboard shortcuts on this machine. The machine runs Hyprland via the ML4W dotfiles framework.

## Where the shortcuts actually live

Keybindings are NOT stored in this repo — this app reads/writes config files in the dotfiles source repo:

```
~/.mydotfiles/com.ml4w.dotfiles/.config/hypr/conf/keybindings/
```

Key files there:
- `default.conf` — the active keybinding set (`bind = $mainMod, KEY, exec, ...` lines)
- `default.lua` — an alternate/newer Lua-based keybinding format (`hl.bind(...)`)
- `fr.conf` — a French keyboard layout variant of the same bindings

Many bindings call small dispatcher scripts in `~/.config/ml4w/settings/*.sh` (symlinked into the dotfiles repo, e.g. `browser.sh`, `editor.sh`, `terminal.sh`) rather than hardcoding a command inline — those scripts just contain the literal command to run. When a keybinding changes an app choice rather than a key, the edit usually belongs in one of these `.sh` files, not in the `.conf`.

After editing `~/.config/hypr/conf/keybindings/*.conf`, Hyprland picks up changes via `hyprctl reload` or on its own file-watch — no need to restart the compositor.

## Build / run

```sh
cargo build
cargo run
```

## Scope note

This app should read and write the dotfiles repo above, but should not assume it owns that repo's git history — treat edits there as touching a separate project.
