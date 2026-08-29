# claude-tray

Which Claude Code sessions are waiting on you, in the system tray.

```text
  ◇        nothing wants you
  ◆ 3      three finished their turn
  ◈ 2      at least one is blocked on you
  ⊘        claude-agents could not be run
```

Click it and the menu lists every live agent; click a row and that agent's zellij pane takes
focus.

It publishes a **StatusNotifierItem** on the session bus, so it lands in Waybar's `tray` module
next to blueman and Telegram with **no Waybar configuration change at all**.

## Why it exists

Running many concurrent Claude Code sessions inside zellij, there is no way to tell which ones
need input and which have finished without visiting each one. This makes that state ambient.

## It does not read the session registry

🔴 **It shells out to [`claude-agents`](https://github.com/lorenzolfm/claude-agents) and parses
its TSV.**

That program already does the pid + `procStart` liveness check that stops a recycled pid from
passing a dead agent off as live, and already joins each agent to its zellij session and pane.
Re-deriving any of it here would create a second source that can disagree with the first — and
`zj-picker` is already the second consumer of the first. One joiner, many consumers.

`claude-agents` is looked up on `PATH`, not pinned, so it can be upgraded underneath the applet.

## What it decides

`claude-agents` passes `status` through verbatim and tells consumers not to match it against a
fixed set. So the mapping lives here, in `src/state.rs`, and nowhere else — it has to be the same
code that draws the badge and builds the menu, or the count and the list could disagree about the
same session.

| raw status | | shown as | counted |
|---|---|---|---|
| `waiting` | | **needs input** ◈ | ✅ |
| `idle`, under an hour in state | | **your turn** ◆ | ✅ |
| `idle`, an hour or more | | idle · | — |
| `idle`, under 30 s since the session started | | idle · | — |
| anything else | | working ○ | — |

Three rules are load-bearing and are each pinned by a test:

- 🔴 **Anything unrecognised is `working`, never actionable.** Version skew reaches the key set,
  not just the values, so a status nobody has seen yet is a matter of time. Failing to *working*
  fails silent; failing to actionable would invent a badge out of a typo.
- 🔴 **The two counted states are asymmetric.** *your turn* ages out after an hour; **needs
  input never ages**, because a session blocked on a prompt does not unblock itself by being
  ignored. Do not tidy them into one threshold.
- 🔴 **Rows that age out are listed, dimmed and uncounted — not hidden.** An hour is only a safe
  threshold because crossing it means *stops nagging*, not *is lost*.

The list is a **pure mirror**. There is no dismiss and no unread; opening the menu resets
nothing. The count is *pending*, always.

Ordering is this program's job: `claude-agents` sorts for clean diffs and says so. Here it is
**actionable first, then oldest first** — within a group, the row waiting longest is the one
ignored longest.

## Colour

The applet sets SNI status `NeedsAttention` whenever the badge is non-zero (and when the producer
is broken — `⊘` with no badge at all is not the same claim as a badge of `0`). The drawn pixmap
stays monochrome, because Waybar's `Item::setStatus` adds a `needs-attention` CSS class and
`AttentionIconPixmap` is an unimplemented TODO. So the cue belongs in `style.css`:

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

Needs `claude-agents` on `PATH` and a session bus. The nix package pins a font carrying `◇◆◈⊘○·`
via `CLAUDE_TRAY_FONT`; a `cargo` build falls back to fontconfig.

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
so both `claude-agents` and `zellij` go missing and the applet shows only `⊘` with a dead jump.
Neither should be pinned to a store path: `claude-agents` so it stays upgradeable underneath, and
`zellij` because the jump talks to a **running server** and a different build would speak to it
wrongly.

⚠️ `%h` is expanded in `ExecStart` but **not** in `Environment=`, so a PATH pointing into `$HOME`
must be written out in full.

## Notes on the rendering

Everything below was observed in a real Waybar 0.15.0, not reasoned about.

- 🔴 **The width budget is free.** Waybar scales a tray pixmap to `icon-size` in *height* and
  preserves the aspect ratio in *width* (`src/modules/sni/item.cpp`, `Item::updateImage`). A tray
  icon is a **height budget, not a 20×20 box** — which is the only reason a glyph *and* a count
  fit.
- 🔴 **Render at the target height, never larger.** An h40 pixmap left for Waybar to downscale
  comes out visibly blurrier than an h20 one.
- 🔴 **`IconName` must stay empty.** Waybar's `getIconPixbuf` prefers the named icon whenever the
  name is non-empty and only then falls back to the pixmap, so a stray name silently discards
  everything drawn.
- 🔴 **Never `Status::Passive`.** Waybar's `show-passive-items` defaults to false and hides
  passive items outright, so a calm applet marked passive would *vanish* rather than sit there
  rendering `◇`.
- ⚠️ **The menu columns line up only because the GTK menu font here is monospace.** That is a
  system setting, not a guarantee.

## Jumping to a pane

The `pane` column is `$ZELLIJ_PANE_ID`, which is exactly what `zellij action focus-pane-id`
takes, addressed at a session by name from outside it. So a click is one process spawn.

⚠️ **It does not raise a window.** Nothing links a Hyprland window to the zellij session running
inside it, so the honest scope is: whichever terminal is already attached moves to the right
pane. Guessing at the window — by title match, or by spawning a second `zellij attach` client —
was considered and rejected as guesswork stacked on a clean primitive.

⚠️ `zellij` exits **0** when the session does not exist, printing the session list instead; only
a bad *pane* id exits non-zero. A successful focus is silent on both streams, so silence is what
this checks.

## Licence

MIT.
