# annotate-linux

Keyboard-driven screen annotation for Wayland compositors with
wlr-layer-shell — Hyprland, Sway, river, niri. A Linux answer to
[Annotate for macOS](https://github.com/epilande/Annotate): toggle a
transparent overlay above every window, draw, present, clear, get out of
the way.

Native Rust + smithay-client-toolkit + cairo. No GTK, no Electron, no
XWayland. Fractional scale (e.g. 1.6) renders pixel-crisp via
`wp_fractional_scale_v1` + `wp_viewporter`.

## Features

- **Tools:** pen, highlighter (self-crossing-safe translucency), line,
  arrow, rectangle, ellipse, numbered counter badges, single-line text
  (click to place, double-click to edit, drag to reposition), select
  (click / marquee, move, copy/cut/paste/duplicate), object eraser
  (sweep removes whole objects; one undo restores the sweep)
- **Constraints:** Shift = square / circle / 45° snapping, Alt =
  center-expand
- **Modes:** persist (default), fade (annotations dissolve after N
  seconds), always-on passthrough (annotations stay visible, clicks and
  keys go to the apps below)
- **UI:** in-overlay toolbar, color palette popup (`c`), width slider
  (`w`, or Ctrl+scroll anywhere), whiteboard/blackboard (`b`) with
  adjustable opacity
- **Cursor:** spotlight highlight, click ripples, drawn cursor styles
  (outline / circle / crosshair)
- **System:** per-output scenes on multi-monitor (hotplug safe), undo/redo
  across everything, state persistence (last tool/color/width/board/fade
  survive restarts), rebindable keys, IPC CLI for scripting

## Install

```sh
# Arch (PKGBUILD in this repo)
makepkg -si

# or plain cargo
cargo build --release
install -Dm755 target/release/annotate-linux ~/.local/bin/annotate-linux
```

Runtime deps: `cairo`, `libxkbcommon`, `wayland`. A compositor speaking
`zwlr_layer_shell_v1` is required (GNOME is not).

## Hyprland setup

```conf
exec-once = annotate-linux daemon        # optional: CLI autostarts it
bind = SUPER,       A, exec, annotate-linux toggle
bind = SUPER SHIFT, A, exec, annotate-linux passthrough
bind = SUPER CTRL,  A, exec, annotate-linux clear
layerrule = noanim, annotate-linux
```

Wayland has no global-hotkey API, so activation goes through your
compositor's binds → the `annotate-linux` CLI → the daemon's socket. The
passthrough bind doubles as the way back out of passthrough (the overlay
takes no input in that mode by design).

## Keys (while the overlay is up)

| Key | Action | Key | Action |
| --- | --- | --- | --- |
| `p` | pen | `s` | select |
| `h` | highlighter | `x` | eraser |
| `l` | line | `c` | color picker |
| `a` | arrow | `w` | width picker |
| `r` | rectangle | `b` | board cycle |
| `e` | ellipse | `Esc` | hide (always) |
| `n` | counter | `Delete` | delete selection |
| `t` | text | `Ctrl+scroll` | width |
| `Ctrl+z` / `Ctrl+Shift+z` | undo / redo | `Ctrl+c/x/v/d` | clipboard |
| `Ctrl+r` | reset counter | | |

Rebind anything in `[keys]` (see
[contrib/config.example.toml](contrib/config.example.toml)).

## CLI

```
annotate-linux toggle | show | hide
              | passthrough [on|off]
              | clear | undo | redo
              | tool <name> | color <#rrggbb|next|prev|N> | width <n|+n|-n>
              | board <none|white|black> [--opacity 0.85]
              | mode <fade|persist> [--seconds 3]
              | cursor <style> [--highlight true]
              | counter-reset | status | reload-config | quit
```

`status` prints JSON. Config lives at
`~/.config/annotate-linux/config.toml`, runtime state at
`~/.local/state/annotate-linux/state.toml`.

## Known limitations (v1)

- Text uses cairo's toy font API: no shaping, no font fallback — CJK,
  RTL, and emoji render as boxes. Pango integration is the planned fix.
- No IME support in the text tool.
- Single-line text only.
- The highlighter approximates marker ink with alpha compositing; a true
  multiply against screen content is impossible from a Wayland overlay
  (clients cannot read what is beneath them).
- In fade mode, a fully faded object remains clickable/erasable for a ~2s
  grace window before garbage collection removes it.

## License

MIT
