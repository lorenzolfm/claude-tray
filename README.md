# claude-tray

Which Claude Code sessions are waiting on you, in the system tray.

```text
  ✻        nothing is blocked on you        (the Claude mark, alone)
  ✻ 2      two agents asked you something   (count in amber)
  ✻ ⊘      claude-ps could not be run       (⊘ in red)
```

**The mark never changes.** It is identity, not state — the same terracotta burst in every
case, so the applet is recognisable at a glance as *the Claude one* rather than as a shape you
have to decode. Everything that varies is the badge beside it.

Click it and the menu lists every live agent; click a row and that agent's zellij pane takes
focus.

It publishes a **StatusNotifierItem** on the session bus, so it lands in Waybar's `tray` module
next to blueman and Telegram with **no Waybar configuration change at all**.

## Why it exists

Running many concurrent Claude Code sessions inside zellij, there is no way to tell which ones
need input and which have finished without visiting each one. This makes that state ambient.

## It does not read the session registry

🔴 **It shells out to [`claude-ps`](https://github.com/lorenzolfm/claude-ps) and reads
the JSON it prints.**

That program already does the pid + `procStart` liveness check that stops a recycled pid from
passing a dead agent off as live, and already joins each agent to its zellij session and pane.
Re-deriving any of it here would create a second source that can disagree with the first — and
[`luneta`](https://github.com/lorenzolfm/luneta) is already the second consumer of the first.
One joiner, many consumers.

`claude-ps` is looked up on `PATH`, not pinned, so it can be upgraded underneath the applet.

## What a row is called

`claude-ps` reports both the agent's `name` and **who chose it**, and the second half is the
load-bearing one. So a row is named in three steps:

1. the name a **person** chose (`name_source` of `user` or `peer`) — the only string on the row
   that says what the agent is *for*;
2. failing that, the **zellij session** it is sitting in, which is the thing you navigate by;
3. failing that, Claude Code's own label, for an agent outside zellij that would otherwise have
   no name at all.

🔴 **A `derived` name is the cwd basename plus a two-character suffix**, so a row that showed it
would be named after a directory it is only incidentally in — and `…/infra.git/master` would read
`master-3c` for a session you reach as `infra`. That is [`luneta`](https://github.com/lorenzolfm/luneta)'s
rule, taken whole.

⚠️ **An unrecognised `name_source` is suppressed**, which is the exact opposite of what an
unrecognised *status* gets. The asymmetry is the producer's and is deliberate on both sides:
every status value is a real state, so hiding one hides a live agent, whereas the sources that
carry a chosen name are a short closed list (`user`, `peer`) and the machinery is the long open
one — Claude Code already writes `derived`, `collision`, `auto` and `hook`. A source invented
tomorrow is far likelier to be more machinery. An **absent** source is trusted, because that is
the state before the key existed rather than one this build failed to recognise.

When two visible rows end up with the same name, **every** one of them takes a `:pane` suffix.
"The first one is bare" is not a rule anyone could read off the menu.

## What it decides

`claude-ps` passes `status` through verbatim and tells consumers not to match it against a
fixed set. So the mapping lives here, in `src/state.rs`, and nowhere else — it has to be the same
code that draws the badge and builds the menu, or the count and the list could disagree about the
same session.

| raw status | | shown as | sorts | counted |
|---|---|---|---|---|
| `waiting` | | 🙋 `waiting` | 1st | ✅ |
| `idle` | | ☕ `idle` | 2nd | — |
| `busy` | | ⣾ `busy` | 3rd | — |
| `shell` | | 🐚 `shell` | last | — |
| anything else | | 🛸 *itself* | last | — |

🔴 **The states are [`luneta`](https://github.com/lorenzolfm/luneta)'s four, and so is the
order** — its agents tab is the standard, its table verbatim, case-insensitivity included, with
nothing added and nothing dropped. The same agent read in the picker and read here has to be the
same picture in the same place; a vocabulary that forked per surface is worse than no vocabulary.

⚠️ **`your turn` and `dormant` used to live here and are gone.** They were this end's own
judgment about `idle`: counted for the first hour, suppressed for the first thirty seconds of a
session's life, quietly filed away after that. Three thresholds, all guesses, none of them
anything the picker knew about — so one agent was *your turn* on one surface and plain `idle` on
the other. The word on a row is now the producer's own, printed as it arrived.

⚠️ One exception, and it is about ink rather than about meaning: the busy cycle here is
`⣾⣽⣻⢿⡿⣟⣯⣷` where the picker's is `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`. Seven dots lit per frame instead of three,
because a menu row is drawn in the GTK theme's foreground and three dots of it read as grey lint
that may or may not be turning. Colour was the smaller fix and is not on offer: Waybar draws this
menu through `libdbusmenu-gtk3`, which `g_markup_escape_text`s every label, so per-glyph markup
arrives as literal angle brackets — and the one colour dbusmenu does expose, `disposition`, paints
the whole row. *Which* status spins is still the picker's call; only the weight of the frames is
this end's.

The spinner really turns, at ten frames a second, and only while the menu is open: `AboutToShow`
is the one signal that says anyone is looking. There is no matching *closed* — `ksni` 0.3.6
routes only `clicked` out of `Event` — so it keeps turning for a minute after the last open and
then stops. Ticking is otherwise free: the producer still runs once every five seconds, and a
tick with no spinner on screen does not even rebuild the menu.

### The badge counts `waiting`, and nothing else

🔴 **This is the one judgment left on top of the picker's order**, and it is the whole reason the
applet is in the bar: `badge > 0` means *somebody is blocked on you*, and a `waiting` agent has
an actual question pending.

`idle` is deliberately not in it. A finished turn is worth a row above the working ones — which
is why it sorts second — but not a number in the bar, because **nothing retires it**. That is
what the old hour-long timeout and newborn suppression were for: stopping a session you finished
with on Tuesday from nagging on Thursday, and stopping a tab you just opened from nagging about
itself. Both were guesses about how long a person stays interested. Counting only `waiting`
needs neither.

So there is one badge colour now, where there were two. Everything in the count is blocked, so
amber says it on its own.

Three rules are load-bearing and are each pinned by a test:

- 🔴 **Anything unrecognised sorts last and is never counted.** The status vocabulary is open and
  moves with Claude Code's releases — the set grew by one (`shell`) between two of them already —
  so a word nobody has seen yet is a matter of time. Ranking it last fails silent; letting it
  count would invent a badge out of a typo. It still renders, **as itself**.
- 🔴 **Case never decides what a status is.** The picker compares every status with
  `eq_ignore_ascii_case`, so this does too — a status that is *recognised* on one surface and
  *unknown* on the other is two mappings again.
- 🔴 **Nothing is ever hidden.** Uncounted is not the same as gone: every live agent gets a row,
  and every row with an address can be clicked. The only dimmed rows are agents outside zellij,
  which have nowhere to send you — where dimmed means **inert**.

The list is a **pure mirror**. There is no dismiss and no unread; opening the menu resets
nothing. The count is *pending*, always.

## Ordering

Ordering is this program's job: `claude-ps` sorts by pid so that two runs a second apart diff
cleanly, and says in its README that this order is for diffing rather than reading.

🔴 **It is the picker's order, both halves**: attention first, and within one status **most
recent first** — the agent that changed a moment ago is the one you were just working with.

⚠️ The second half is a reversal. This menu used to put the *oldest* row of a group on top, on
the reasoning that it had been ignored longest. The picker's reasoning won because the picker is
where the navigating actually happens, and two surfaces that disagree about which row is at the
top are worse than either rule on its own. Ties fall back to the producer's pid order, so two
agents that changed in the same second do not swap places between polls.

Rows are one flat list, as in the picker. The dividers that used to separate *wants you* from
*is running* from *has aged out* went with the states they separated; the glyph and the word
already say which block a row is in.

## The age column moves

Every row ends in how long the agent has been in its current status — `<1m`, `47m`, `3h`, `2d`.
On a `busy` row that is turn duration, and it is the only surface a wedged session shows up on
at all.

🔴 **It is the snapshot's age plus how long ago the snapshot was taken.** The list is a glance:
`claude-ps` runs once every five seconds and once more on the way to opening the menu, and the
menu is then rebuilt ten times a second off that frozen answer. Without the offset an agent that
has been waiting three minutes would read the same number for as long as you looked at it — on
the one column that says whether anything is stuck.

⚠️ Adding it is safe *because* it is the same number on every row: a uniform offset cannot flip
a comparison, so the ordering above lands exactly where it did and only the ages move.

## The mark

`assets/claude-mark.svg` is Claude's own `favicon.svg`, vendored verbatim: one closed path in a
248×248 box, filled `#D97757`. `src/mark.rs` rasterises it at exactly `icon-size` with a scanline
filler — supersampled in `y`, analytic in `x` — rather than shipping a bitmap.

🔴 **That is not gold-plating, it is the cheaper option.** A committed PNG would be one fixed
size that Waybar then resamples, and Waybar's downscale is visibly blurrier than a native render
(see *Notes on the rendering*). Rasterising instead costs ~90 lines, no dependency, and stays
crisp at any `icon-size`. It is checked against the artwork it imitates: Claude's `favicon.ico`
ships the mark pre-rendered at 48, 32 and 16 px, all three inking **0.3589** of their box, and
the filler lands within 0.002 of that at every size.

## Colour

Three colours, each meaning exactly one thing:

| | | |
|---|---|---|
| `#D97757` | the mark, always | identity — it never changes |
| `#e5c07b` | the count | *n* sessions are **blocked on you** |
| `#e06c75` | `⊘` | the applet **cannot see** — `claude-ps` is missing or failing |

🔴 **The pixmap used to be monochrome so that colour could live in `style.css`. That reason is
gone** — see the warning below: a tray item is a `Gtk::Image` and `color` does nothing to it. So
colour is in the pixels or it is nowhere. Amber is what the retired `◈` glyph used to say.

⚠️ There was a fourth, `#fdf6e3`, for a count of turns that had merely *finished*. It went with
`your turn`: the number has one meaning now, so it has one colour. 🔴 If a second one is ever
wanted, note what was already learnt about the first — the mark's own terracotta was tried for
the count and it is the dimmest thing on the bar, which is backwards. The mark is the part you
already know; the number is the part you have to read.

The applet also sets SNI status `NeedsAttention` whenever the badge is non-zero (and when the
producer is broken — `⊘` with no badge at all is not the same claim as a badge of `0`), which
Waybar turns into a `needs-attention` CSS class. That is the second cue, and it can only be a
border:

```css
#tray .needs-attention {
  border-bottom: 2px solid #e5c07b;
}
```

⚠️ **`color` does nothing here, and this is worth knowing before you reach for it.** A tray item
is a `Gtk::Image`, so `color` — which styles *text* — never touches the ink. Probed in a real
bar: `background-color` applies and fills the whole cell, a border applies, `color` is silently
ignored. Any cue has to be a property of the box.

## Install

```sh
nix profile install github:lorenzolfm/claude-tray
```

Needs `claude-ps` on `PATH` and a session bus. The mark needs no font — it is rasterised
from the vendored SVG — but the badge does, so the nix package pins one carrying `⊘` and the
digits via `CLAUDE_TRAY_FONT`; a `cargo` build falls back to fontconfig.

The **menu** is drawn by the tray host, not by this program, so `CLAUDE_TRAY_FONT` does not
reach it: its 🙋 ☕ 🐚 🛸 want a colour emoji font on the box (Pango resolves the `emoji` family
for them, which on NixOS means `noto-fonts-color-emoji`), and the braille spinner wants any of
the usual text fonts. The columns line up only if the emoji come out two cells wide against a
monospace menu font — best effort, as it was before, and a system setting either way.

## Autostart

It does **not** need to start after the bar. `ksni` is built with `assume_sni_available(true)`, so
a missing `org.kde.StatusNotifierWatcher` is a wait rather than an error, and the item registers
itself whenever a host appears — at login and after every bar restart alike. `journalctl --user -u
claude-tray` distinguishes the two invisible states:

```
claude-tray: no tray host (…), waiting for one
claude-tray: tray host appeared, item registered
```

⚠️ **This matters more than it looks.** Waybar is typically a compositor `exec-once`, so it starts
*after* the systemd user manager — a unit ordered "after the tray host" is not merely awkward, it is
impossible. Without `assume_sni_available` the applet exits 1 at every login, and systemd's default
start limit then leaves it dead for good.

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

⚠️ **On NixOS, set the unit's `PATH` explicitly.** NixOS gives user units a sanitised environment
holding only coreutils, findutils, grep, sed and systemd — it overrides the user manager's own PATH,
so `claude-ps`, `zellij`, `ss` and `hyprctl` go missing and the applet shows only `⊘` with a dead
jump.
Neither should be pinned to a store path: `claude-ps` so it stays upgradeable underneath, and
`zellij` because the jump talks to a **running server** and a different build would speak to it
wrongly.

⚠️ `%h` is expanded in `ExecStart` but **not** in `Environment=`, so a PATH pointing into `$HOME`
must be written out in full.

⚠️ **Do not try to fix the jump by adding `HYPRLAND_INSTANCE_SIGNATURE` to the unit.** It cannot
be known at the time the unit is written — the applet resolves it at every call instead. See
*Jumping to a pane*.

## Notes on the rendering

Everything below was observed in a real Waybar 0.15.0, not reasoned about.

- 🔴 **The width budget is free.** Waybar scales a tray pixmap to `icon-size` in *height* and
  preserves the aspect ratio in *width* (`src/modules/sni/item.cpp`, `Item::updateImage`). A tray
  icon is a **height budget, not a 20×20 box** — which is the only reason the mark *and* a count
  fit side by side.
- 🔴 **Render at the target height, never larger.** An h40 pixmap left for Waybar to downscale
  comes out visibly blurrier than an h20 one.
- 🔴 **`IconName` must stay empty.** Waybar's `getIconPixbuf` prefers the named icon whenever the
  name is non-empty and only then falls back to the pixmap, so a stray name silently discards
  everything drawn.
- 🔴 **Never `Status::Passive`.** Waybar's `show-passive-items` defaults to false and hides
  passive items outright, so a calm applet marked passive would *vanish* rather than sit there
  showing the bare mark.
- ⚠️ **Straight alpha, not premultiplied.** Waybar hands the ARGB32 buffer to a `GdkPixbuf`,
  which is non-premultiplied. ⚠️ And the bytes go `A, R, G, B` — network order, not the
  little-endian `B, G, R, A` an in-memory `u32` would give you.
- ⚠️ **The menu columns line up only because the GTK menu font here is monospace.** That is a
  system setting, not a guarantee.

## Jumping to a pane

Clicking a row puts you in front of that agent. 🎯 **One terminal, many sessions** — so the rule
is *retarget the terminal you already have*, and open one only when there is none at all. There
is no which-window arbitration because there is nothing to arbitrate.

`zellij.pane` is `$ZELLIJ_PANE_ID`, which is exactly what `zellij action focus-pane-id` takes,
addressed at a session by name from outside it. The producer nests it with `zellij.session` in
one object, or emits `null` — so a row either has a whole address or none, and there is no
half-answer for the jump to guard against.

🔴 **A zellij client's `argv` names the session it originally attached to, not the one it is
showing.** The live session comes from the socket instead: a client is connected to exactly one
`zellij --server <path>/<session>`, so pairing the two ends of that socket names the session as
fact. ⚠️ `/proc/net/unix` does not carry peer inodes — only `ss` does, over sock_diag netlink —
which is why the jump shells out for it.

🔴 **`switch-session` is delivered to the last client that pressed a key in that session**, and a
client that arrived by *switching* has pressed none. So the terminal is woken with a `Ctrl e`
pair — a binding zellij swallows whole, so it never reaches the pane — and the switch is then
**verified** by reading the client's live session back, rather than trusted to its exit code.

⚠️ `zellij` exits **0** when the session does not exist, printing the session list instead; only
a bad *pane* id exits non-zero. A successful focus is silent on both streams, so silence is what
this checks.

### `hyprctl` needs a signature this process cannot inherit

🔴 **This is what made every click a no-op**, and it is worth reading before touching the unit
file. The jump asks Hyprland which pids own windows and then raises one, and `hyprctl` finds the
compositor through `HYPRLAND_INSTANCE_SIGNATURE`. systemd brings the user session up at boot and
Hyprland is an ordinary process *inside* it, so the unit's environment is fixed before the
compositor exists and never learns the signature. An `import-environment` would have to run after
the applet rather than before it.

⚠️ **And it failed silently.** `hyprctl` prints `HYPRLAND_INSTANCE_SIGNATURE not set!` **on
stdout** and exits **0** — the exit code says success and the answer is prose. That is why
nothing in the jump reads an exit code from it: every call matches on what came back instead,
and prose is not a window list.

So the applet resolves the signature itself, off `$XDG_RUNTIME_DIR/hypr`: one directory per
instance, named by the signature. ⚠️ **A directory is not an instance — a socket is.** Hyprland
leaves the directory and its log behind when it exits, so a box that has been through a
compositor restart has several and only one can still be spoken to; requiring `.socket.sock`
drops the corpses, and the newest of what survives is the live one. An environment that *does*
carry the variable always wins, since that is the compositor the person is actually looking at.

## Licence

MIT.
