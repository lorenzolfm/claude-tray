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

## It does not read the session registry

The program runs [`claude-ps`](https://github.com/lorenzolfm/claude-ps) and reads the JSON that
it prints.

That program already does the pid and `procStart` liveness check that prevents a recycled pid
from showing a dead agent as live, and it already joins each agent to its zellij session and
pane. A second implementation here could disagree with the first, and
[`luneta`](https://github.com/lorenzolfm/luneta) is already the second consumer of the first. One
joiner serves many consumers.

`claude-ps` comes from `PATH` and is not pinned, so you can upgrade it without a rebuild of the
applet.

## What a row is called

`claude-ps` reports the `name` of the agent and who chose it, and the second value is the
important one. A row thus takes a name in three steps:

1. the name that a person chose (a `name_source` of `user` or `peer`), which is the only string
   in the row that gives the purpose of the agent;
2. or the zellij session that holds the agent, which is what you navigate by;
3. or Claude Code's own label, for an agent outside zellij that would otherwise have no name.

A `derived` name is the cwd basename plus a two-character suffix. A row with that name would
carry the name of a directory that holds it by chance: `…/infra.git/master` would show
`master-3c` for a session that you reach as `infra`. This rule comes from
[`luneta`](https://github.com/lorenzolfm/luneta).

An unknown `name_source` is suppressed, which is the opposite of the treatment of an unknown
status. The producer causes that difference, and it is correct on both sides. Each status value
is a real state, so a hidden value hides a live agent. But the sources that carry a chosen name
are a short closed list (`user`, `peer`), and the sources that carry a generated name are a long
open one: Claude Code already writes `derived`, `collision`, `auto` and `hook`. A new source is
more probably a generated name. An absent source is trusted, because it is the state from before
the key existed and not a value that this build failed to recognise.

When two rows in the menu take the same name, each of those rows takes a `:pane` suffix. A rule
that leaves the first row without one is not a rule that a person can see in the menu.

## What it decides

`claude-ps` passes `status` through unchanged and tells consumers not to compare it against a
fixed set. The mapping thus lives in `src/state.rs` and in no other file. The code that draws the
badge must be the code that builds the menu, or the count and the list can disagree about one
session.

| raw status | | shown as | sorts | counted |
|---|---|---|---|---|
| `waiting` | | 🙋 `waiting` | 1st | ✅ |
| `idle` | | ☕ `idle` | 2nd | — |
| `busy` | | ⣾ `busy` | 3rd | — |
| `shell` | | 🐚 `shell` | last | — |
| anything else | | 🛸 *itself* | last | — |

The states and the order come from [`luneta`](https://github.com/lorenzolfm/luneta). Its agents
tab is the standard, and this program uses its table without a change, including the comparison
that ignores case. The same agent in the picker and here must give the same picture in the same
place, and a vocabulary that differs between two surfaces is worse than no vocabulary.

An earlier design added two states of its own, `your turn` and `dormant`, which held this
program's own decisions about `idle`: counted for the first hour, suppressed for the first thirty
seconds of the life of a session, and hidden after that. Those three thresholds were estimates,
and the picker knew none of them, so one agent was `your turn` on one surface and `idle` on the
other. The word in a row now comes from the producer, unchanged.

There is one exception, and it concerns ink and not meaning. The busy cycle here is `⣾⣽⣻⢿⡿⣟⣯⣷`
where the picker's is `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`: seven dots in each frame instead of three. The GTK theme
draws a menu row in its own foreground colour, and three dots become grey specks that may or may
not turn. Colour is the smaller change and is not available: Waybar draws this menu through
`libdbusmenu-gtk3`, which calls `g_markup_escape_text` on each label, so markup for one glyph
shows as angle brackets. The one colour that dbusmenu gives, `disposition`, paints the full row.
Which status turns is still the picker's decision, and only the weight of the frames belongs to
this program.

The spinner turns at ten frames a second, and only while the menu is open: `AboutToShow` is the
one signal that says that a person looks at the menu. There is no matching signal for a close,
because `ksni` 0.3.6 sends only `clicked` out of `Event`, so the spinner turns for one minute
after the last open and then stops. Each other tick is cheap: the producer still runs once every
five seconds, and a tick with no spinner on screen does not rebuild the menu.

### The badge counts `waiting`, and nothing else

This is the one decision that this program adds to the picker's order, and it is the reason for
the applet: `badge > 0` means that an agent waits for you, and a `waiting` agent has a question
pending.

The badge does not count `idle`. A finished turn is sufficient for a row above the working rows,
which is why it sorts second, but it is not sufficient for a number in the bar, because nothing
ends that state. That is what the old timeout of one hour and the old delay for new sessions
were for: to keep a session that you finished on Tuesday quiet on Thursday, and to keep a session
that you just opened quiet about itself. Both values were estimates of how long a person stays
interested. A count of `waiting` alone needs neither.

There is thus one badge colour where there were two. Each agent in the count waits for you, so
amber alone reports it.

Three rules are important, and a test verifies each one:

- An unknown status sorts last, and the badge never counts it. The status vocabulary is open and
  changes with the releases of Claude Code, which added `shell` between two of them. A new word
  is thus only a question of time. Last place fails quietly, but a count would make a badge out
  of a spelling error. The menu still shows the row, with the status itself as its word.
- Case never decides what a status is. The picker compares each status with
  `eq_ignore_ascii_case`, so this program does the same. A status that one surface recognises and
  the other does not is a second mapping.
- The menu hides nothing. Uncounted is not absent: each live agent takes a row, and you can click
  each row that has an address. The only grey rows are agents outside zellij, which have no
  destination, and grey there means that the row does nothing.

The list is a mirror. There is no dismiss and no unread, and an open of the menu changes nothing.
The count is always the pending work.

## Ordering

The order is this program's responsibility. `claude-ps` sorts by pid, so that two runs one second
apart give a clean difference, and its README says that this order is for comparison and not for
reading.

The order comes from the picker, in both parts: attention first, and most recent first within one
status. The agent that changed a moment ago is the agent that you last worked with.

The second part is a reversal. This menu put the oldest row of a group first, because that row
had waited longest. The picker's rule won, because the picker is where the navigation happens and
two surfaces that disagree about the first row are worse than either rule alone. Equal ages keep
the producer's pid order, so two agents that changed in the same second do not exchange places
between polls.

The rows are one flat list, as in the picker. There are no dividers between groups, because the
glyph and the word already show the group of a row.

## The age column moves

Each row ends with the time that the agent has been in its current status: `<1m`, `47m`, `3h`,
`2d`. On a `busy` row that is the duration of the turn, and it is the only indication that a
session is stuck.

The value is the age in the snapshot plus the time since the snapshot. `claude-ps` runs once
every five seconds and once more as the menu opens, and the menu then rebuilds ten times a second
from that one answer. Without the offset, an agent that has waited three minutes would show the
same number for as long as you look at it, in the one column that reports whether an agent is
stuck.

The addition is safe because it is the same number in each row. An equal offset cannot change a
comparison, so the order above stays the same and only the ages move.

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

A click on a row moves you to that agent. There is one terminal and many sessions, so the rule is
to change the session in the terminal that you have, and to open a terminal only if there is
none. There is no rule to select between windows, because there is nothing to select.

`zellij.pane` is `$ZELLIJ_PANE_ID`, which is what `zellij action focus-pane-id` takes, addressed
at a session by name from outside it. The producer sends it with `zellij.session` in one object,
or it sends `null`. A row thus has a full address or none, and the jump has no half-answer to
handle.

The `argv` of a zellij client names the session that it attached to first, not the session that it
shows now. The live session comes from the socket instead: a client connects to exactly one
`zellij --server <path>/<session>`, so a pair of the two ends of that socket gives the session.
`/proc/net/unix` does not hold peer inodes, and only `ss` does, over sock_diag netlink, so the
jump runs `ss`.

That pairing splits the rows of `ss -x -p` on whitespace, which holds for each socket path that
zellij builds and not for each session name that a person types. A session named `my work` is
therefore joined to no client: the jump finds no terminal showing it and falls through to
`attach`, which opens a *second* terminal for a session that is already on screen, and the click
looks like it worked. The applet refuses such an address instead. A session or pane that is empty
or holds whitespace is not addressable, so its row is grey and inert. It keeps its name, and only
the jump is lost — which was already lost, silently, plus a stray terminal.

zellij sends `switch-session` to the last client that pressed a key in that session, and a client
that arrived by a change of session has pressed none. The jump therefore sends a `Ctrl e` pair,
which is a binding that zellij consumes and which never reaches the pane, and it then verifies
the change: it reads the live session of the client again instead of trust in the exit code.

`zellij` exits 0 when the session does not exist and prints the session list instead. Only a
wrong pane id exits non-zero. A focus that succeeds is silent on both streams, so this code tests
for silence.

### `hyprctl` needs a signature that this process cannot inherit

This failure stopped each click, and you must read it before you change the unit file. The jump
asks Hyprland which pids own windows and then raises one, and `hyprctl` finds the compositor
through `HYPRLAND_INSTANCE_SIGNATURE`. systemd starts the user session at boot, and Hyprland is a
process inside it, so the environment of the unit is fixed before the compositor exists and never
receives the signature. An `import-environment` would have to run after the applet and not before
it.

The failure was also silent. `hyprctl` prints `HYPRLAND_INSTANCE_SIGNATURE not set!` on stdout
and exits 0: the exit code reports success, and the answer is text. No code in the jump thus reads
an exit code from it. Each call examines the output instead, and text is not a window list.

The applet therefore finds the signature itself, in `$XDG_RUNTIME_DIR/hypr`, which holds one
directory for each instance with the signature as its name. A directory is not an instance, but a
socket is. Hyprland leaves the directory and its log after it exits, so a machine with a restart
of the compositor has several directories and only one of them accepts a connection. A test for
`.socket.sock` removes the old directories, and the newest of the remainder is the live one. An
environment that has the variable always wins, because that is the compositor that the person
looks at.

## Licence

MIT.
