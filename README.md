# claude-tray

Which Claude Code sessions are waiting on you, in the system tray.

```text
  ✻        nothing wants you                (the Claude mark, alone)
  ✻ 3      three finished their turn        (count in the bar's foreground)
  ✻ 2      at least one is blocked on you   (count in amber)
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
`zj-picker` is already the second consumer of the first. One joiner, many consumers.

`claude-ps` is looked up on `PATH`, not pinned, so it can be upgraded underneath the applet.

## The token count on each row

`claude-ps` reports how much context each agent was carrying at its last assistant turn, and
each menu row carries it as a trailing `188k`.

⚠️ **Tokens, never a percentage.** The context window *size* is never written to disk — Claude
Code computes it and hands it to a status line at render time — so a denominator here would have
to come from a model-name table that goes confidently wrong the day a new model ships. That is
the same failure this applet avoids by passing `status` through, and worse: an unrecognised
status renders as itself, while a wrong denominator renders as a number that looks right.

A row whose count is missing simply **omits** it. The producer's join for this one key is a path
derived from `cwd` rather than a proof, so "not known" is ordinary — and a `0` there would be a
lie the eye cannot catch.

## What it decides

`claude-ps` passes `status` through verbatim and tells consumers not to match it against a
fixed set. So the mapping lives here, in `src/state.rs`, and nowhere else — it has to be the same
code that draws the badge and builds the menu, or the count and the list could disagree about the
same session.

| raw status | | shown as | counted |
|---|---|---|---|
| `waiting` | | 🙋 **needs input** | ✅ |
| `idle`, under an hour in state | | ☕ **your turn** | ✅ |
| `idle`, an hour or more | | ☕ idle | — |
| `idle`, under 30 s since the session started | | ☕ idle | — |
| `busy` | | ⣾ working | — |
| `shell` | | 🐚 working | — |
| anything else | | 🛸 working | — |

🔴 **The glyph is the status, and `zj-picker`'s agents tab is the standard for it** — its table
verbatim, case-insensitivity included, with nothing added and nothing dropped. The same agent
read in the picker and read here has to be the same picture; a vocabulary that forked per
surface would be worse than no vocabulary.

⚠️ One exception, and it is about ink rather than about meaning: the busy cycle here is
`⣾⣽⣻⢿⡿⣟⣯⣷` where the picker's is `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`. Seven dots lit per frame instead of three,
because a menu row is drawn in the GTK theme's foreground and three dots of it read as grey lint
that may or may not be turning. Colour was the smaller fix and is not on offer: Waybar draws this
menu through `libdbusmenu-gtk3`, which `g_markup_escape_text`s every label, so per-glyph markup
arrives as literal angle brackets — and the one colour dbusmenu does expose, `disposition`, paints
the whole row. *Which* status spins is still the picker's call; only the weight of the frames is
this end's.

So the glyph carries the producer's word and *only* that. Everything this program decides on top
of it — counted or not, *your turn* or aged out — is in the word beside the glyph and the block
the row sits in. Two rows both reading ☕ are two `idle` agents, and the one under the divider is
the one that stopped nagging.

The spinner really turns, at ten frames a second, and only while the menu is open: `AboutToShow`
is the one signal that says anyone is looking. There is no matching *closed* — `ksni` 0.3.6
routes only `clicked` out of `Event` — so it keeps turning for a minute after the last open and
then stops. Ticking is otherwise free: the producer still runs once every five seconds, and a
tick with no spinner on screen does not even rebuild the menu.

Three rules are load-bearing and are each pinned by a test:

- 🔴 **Anything unrecognised is `working`, never actionable.** Version skew reaches the key set,
  not just the values, so a status nobody has seen yet is a matter of time. Failing to *working*
  fails silent; failing to actionable would invent a badge out of a typo.
- 🔴 **Case never decides what a status is.** `zj-picker` compares every status with
  `eq_ignore_ascii_case`, so this does too — a status that is *recognised* on one surface and
  *unknown* on the other is two mappings again, and the unknown side is the uncounted one, so
  the skew would hide a row rather than merely mislabel it.
- 🔴 **The two counted states are asymmetric.** *your turn* ages out after an hour; **needs
  input never ages**, because a session blocked on a prompt does not unblock itself by being
  ignored. Do not tidy them into one threshold.
- 🔴 **Rows that age out are listed, dimmed and uncounted — not hidden.** An hour is only a safe
  threshold because crossing it means *stops nagging*, not *is lost*.

The list is a **pure mirror**. There is no dismiss and no unread; opening the menu resets
nothing. The count is *pending*, always.

Ordering is this program's job: `claude-ps` sorts for clean diffs and says so. Here it is
**actionable first, then oldest first** — within a group, the row waiting longest is the one
ignored longest.

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
| `#fdf6e3` | the count | *n* turns have finished |
| `#e5c07b` | the count | at least one session is **blocked on you** |
| `#e06c75` | `⊘` | the applet **cannot see** — `claude-ps` is missing or failing |

🔴 **The pixmap used to be monochrome so that colour could live in `style.css`. That reason is
gone** — see the warning below: a tray item is a `Gtk::Image` and `color` does nothing to it. So
colour is in the pixels or it is nowhere. Amber over off-white is what the retired `◈`-over-`◆`
glyph pair used to say, and `#fdf6e3` is not decoration either: the mark's own terracotta was
tried for the count and it is the dimmest thing on the bar, which is backwards. The mark is the
part you already know; the number is the part you have to read.

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
