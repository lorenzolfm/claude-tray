# claude-tray

Which Claude Code sessions wait for you, in the system tray.

```text
  ✻        nothing waits for you            (the Claude mark, alone)
  ✻ 2      two agents asked a question      (count in amber)
  ✻ ⊘      claude-ps could not run          (⊘ in red)
```

The mark does not change. It is identity and not state: the same terracotta burst in each
condition, so that you recognise the applet as the Claude one instead of a shape that you must
decode. Only the badge beside it changes.

A click shows a menu of each live agent. A click on a row moves the focus to the zellij pane of
that agent.

The program publishes a **StatusNotifierItem** on the session bus, so it appears in Waybar's
`tray` module beside blueman and Telegram with no change to the Waybar configuration.

## Why it exists

With many concurrent Claude Code sessions inside zellij, there is no way to see which sessions
need input and which have finished without a visit to each one. This program shows that state
in the bar.

## It decides nothing about the agents

The program links [`claude-nav`](https://github.com/lorenzolfm/claude-nav), which runs
[`claude-ps`](https://github.com/lorenzolfm/claude-ps) and decides what its answer means: what
each row is called, which status is `waiting`, where a row sorts, and where a click goes.

Both halves of that are shared on purpose, and neither one is shared for tidiness.

`claude-ps` already does the pid and `procStart` liveness check that prevents a recycled pid from
showing a dead agent as live, and it already joins each agent to its zellij session and pane. One
joiner serves many consumers.

`claude-nav` is the second layer of the same argument. [`luneta`](https://github.com/lorenzolfm/luneta)'s
agents tab, this applet and the vicinae extension are three surfaces that must give one picture:
the same agent, in the same place, with the same word. The vocabulary and the jump therefore live
in one crate, and this repository holds no copy of either. When an earlier version of this
program did add two states of its own, the same agent read as `your turn` here and `idle` in the
picker.

Neither program is pinned by the nix package. `claude-ps` comes from `PATH`, so you can upgrade it
without a rebuild of the applet; `claude-nav` is compiled in, because a menu row is a Rust value
here and not a line of JSON.

## What is left to decide

Three things, and each one is ink:

| | |
|---|---|
| the badge | what the count means, and its colour |
| `src/menu.rs` | the column width, and a spinner heavy enough for a GTK menu |
| `src/mark.rs`, `src/icon.rs` | the mark, rasterised at `icon-size` |

Everything else — the 🙋 ☕ 🐚 🛸, the order of the rows, the `<1m` / `47m` / `3h` / `2d` of the
age column, which rows are grey — is `claude-nav`'s answer, rendered.

### The badge counts `waiting`, and nothing else

This is the one decision that this program adds to the picker's order, and it is the reason for
the applet: `badge > 0` means that an agent waits for you, and a `waiting` agent has a question
pending.

The badge does not count `idle`, at any age. That rule is
[`claude-nav`'s](https://github.com/lorenzolfm/claude-nav#waiting-is-the-only-status-that-counts)
and the reasoning is there; what belongs here is the consequence. There is one badge colour where
there were two, because the number has one meaning: each agent in the count waits for you, so
amber alone reports it.

The menu hides nothing. Uncounted is not absent: each live agent takes a row, and you can click
each row that has an address. The only grey rows are agents outside zellij, which have no
destination, and grey there means that the row does nothing.

The list is a mirror. There is no dismiss and no unread, and an open of the menu changes nothing.
The count is always the pending work.

## The spinner is heavier here

`claude-nav` says which status turns and does not say how many dots turn, and this is the one
place where this applet differs from the picker on purpose.

The busy cycle here is `⣾⣽⣻⢿⡿⣟⣯⣷` where the picker's is `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`: seven dots in each frame
instead of three. The GTK theme draws a menu row in its own foreground colour, and three dots
become grey specks that may or may not turn. Colour is the smaller change and is not available:
Waybar draws this menu through `libdbusmenu-gtk3`, which calls `g_markup_escape_text` on each
label, so markup for one glyph shows as angle brackets. The one colour that dbusmenu gives,
`disposition`, paints the full row.

The spinner turns at ten frames a second, and only while the menu is open: `AboutToShow` is the
one signal that says that a person looks at the menu. There is no matching signal for a close,
because `ksni` 0.3.6 sends only `clicked` out of `Event`, so the spinner turns for one minute
after the last open and then stops. Each other tick is cheap: the producer still runs once every
five seconds, and a tick with no spinner on screen does not rebuild the menu.

## The age column moves

Each row ends with the time that the agent has been in its current status. The value is the age
in the snapshot plus the time since the snapshot, because `claude-ps` runs once every five
seconds and once more as the menu opens, and the menu then rebuilds ten times a second from that
one answer. Without the offset, an agent that has waited three minutes would show the same number
for as long as you look at it, in the one column that reports whether an agent is stuck.

The addition is safe because it is the same number in each row. An equal offset cannot change a
comparison, so the order stays the same and only the ages move.

## The mark

`assets/claude-mark.svg` is Claude's own `favicon.svg`, copied without a change: one closed path
in a 248×248 box, filled `#D97757`. `src/mark.rs` rasterises it at exactly `icon-size` with a
scanline filler, which supersamples along `y` and computes the overlap along `x`.

That is the cheaper option and not an extra. A committed PNG would have one size, and Waybar
would resample it, and Waybar's scale-down is more blurred than a native render (see *Notes on
the rendering*). The rasteriser costs about 90 lines and no dependency, and it stays sharp at each
`icon-size`. A test compares it against the source artwork: Claude's `favicon.ico` holds the mark
at 48, 32 and 16 px, each one covers **0.3589** of its box, and the filler stays within 0.002 of
that value at each size.

## Colour

Three colours, and each one has one meaning:

| | | |
|---|---|---|
| `#D97757` | the mark, always | identity, and it never changes |
| `#e5c07b` | the count | *n* sessions wait for you |
| `#e06c75` | `⊘` | the applet cannot see: `claude-ps` is absent or it fails |

The colour is in the pixels, because CSS cannot supply it. See the warning below: a tray item is
a `Gtk::Image`, and `color` has no effect on it.

An earlier design had a fourth colour, `#fdf6e3`, for a count of turns that had finished. It went
with the `your turn` state: the number now has one meaning, so it has one colour. If a second
colour becomes necessary, note the result of an earlier test: the terracotta of the mark is the
least visible colour on the bar, which is the wrong result. You already know the mark, and you
must read the number.

The applet also sets the SNI status `NeedsAttention` while the badge is non-zero, and while the
producer fails. (A `⊘` with no badge is a different statement from a badge of `0`.) Waybar turns
that status into a `needs-attention` CSS class, which is the second signal and can only be a
border:

```css
#tray .needs-attention {
  border-bottom: 2px solid #e5c07b;
}
```

`color` has no effect here, and you must know that before you use it. A tray item is a
`Gtk::Image`, so `color`, which styles text, never touches the ink. Measured in a real bar:
`background-color` applies and fills the full cell, a border applies, and Waybar ignores `color`.
A signal must thus be a property of the box.

## Install

```sh
nix profile install github:lorenzolfm/claude-tray
```

The program needs `claude-ps` on `PATH` and a session bus. The mark needs no font, because
`src/mark.rs` rasterises it from the SVG. The badge does need one, so the nix package pins a font
that carries `⊘` and the digits, through `CLAUDE_TRAY_FONT`. A `cargo` build uses fontconfig
instead.

The tray host draws the menu, and this program does not, so `CLAUDE_TRAY_FONT` does not reach it.
Its 🙋 ☕ 🐚 🛸 need a colour emoji font on the machine. Pango resolves the `emoji` family for
them, which on NixOS means `noto-fonts-color-emoji`. The braille spinner needs one of the usual
text fonts. The columns align only if the emoji are two cells wide against a monospace menu font,
which is a system setting.

## Autostart

The program does not need to start after the bar. `ksni` runs with `assume_sni_available(true)`,
so an absent `org.kde.StatusNotifierWatcher` is a wait and not an error, and the item registers
itself when a host appears, at login and after each restart of the bar. `journalctl --user -u
claude-tray` separates the two invisible states:

```
claude-tray: no tray host (…), waiting for one
claude-tray: tray host appeared, item registered
```

This is more important than it appears. Waybar is usually a compositor `exec-once`, so it starts
after the systemd user manager. A unit ordered after the tray host is thus impossible. Without
`assume_sni_available`, the applet exits 1 at each login, and the default start limit of systemd
then leaves it dead.

A minimal user unit:

```ini
[Unit]
Description=Claude Code session tray applet
Requires=dbus.socket
After=dbus.socket
StartLimitIntervalSec=0

[Service]
ExecStart=%h/.nix-profile/bin/claude-tray
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
```

On NixOS, set the `PATH` of the unit. NixOS gives a user unit a limited environment that holds
only coreutils, findutils, grep, sed and systemd, and it replaces the PATH of the user manager.
`claude-ps`, `zellij`, `ss` and `hyprctl` are then absent, and the applet shows `⊘` and cannot
jump. Do not pin `claude-ps` or `zellij` to a store path: `claude-ps` must stay upgradeable, and
the jump speaks to a running `zellij` server, which a different build could answer incorrectly.

systemd expands `%h` in `ExecStart` but not in `Environment=`, so write a PATH that points into
`$HOME` in full.

Do not add `HYPRLAND_INSTANCE_SIGNATURE` to the unit to repair the jump. Its value is not known
when you write the unit, and the applet finds it at each call instead. See *Jumping to a pane*.

## Notes on the rendering

Each item below was measured in a real Waybar 0.15.0.

- The width costs nothing. Waybar scales a tray pixmap to `icon-size` in height and keeps the
  aspect ratio in width (`src/modules/sni/item.cpp`, `Item::updateImage`). A tray icon is thus a
  height limit and not a 20×20 box, which is the reason that the mark and a count fit side by
  side.
- Render at the target height and never larger. Waybar scales an h40 pixmap down, and the result
  is visibly more blurred than an h20 one.
- `IconName` must stay empty. Waybar's `getIconPixbuf` returns the named icon while the name has
  a value, and it uses the pixmap only if that name is empty, so a name removes each drawn pixel.
- Never use `Status::Passive`. Waybar's `show-passive-items` is false by default and it hides a
  passive item, so a quiet applet with that status would disappear instead of a display of the
  mark alone.
- Use straight alpha and not premultiplied alpha, because Waybar sends the ARGB32 buffer to a
  `GdkPixbuf`. The bytes are `A, R, G, B`, which is network order and not the little-endian
  `B, G, R, A` of a `u32` in memory.
- The menu columns align only because the GTK menu font here is monospace, which is a system
  setting and not a guarantee.

## Jumping to a pane

A click on a row calls `claude_nav::jump::focus`, which changes the session in the terminal that
you have and opens one only if there is none. The four silent failures it was written against —
the `argv` of a zellij client naming the wrong session, a session name with whitespace, a
`switch-session` that reaches a client which has pressed no key, and a `hyprctl` that cannot
inherit `HYPRLAND_INSTANCE_SIGNATURE` — are documented
[there](https://github.com/lorenzolfm/claude-nav#jumping-to-a-pane).

One of them reaches this repository, and it is the reason for the paragraph about `PATH` under
*Autostart*: the jump needs `zellij`, `ss`, `hyprctl` and a terminal, and a systemd user unit on
NixOS has none of them.

## Licence

MIT.
