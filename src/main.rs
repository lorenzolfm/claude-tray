//! **claude-tray** — which Claude Code sessions are waiting on you, in the system tray.
//!
//! Lorenzo runs many concurrent Claude Code sessions inside zellij and cannot tell which ones
//! need input or have finished. This publishes a StatusNotifierItem so that state is ambient:
//! the Claude mark and a count always in the bar, the session list one click away.
//!
//! ```text
//!   ✻        nothing is blocked on you          (the mark, alone)
//!   ✻ 2      two agents asked you something     (count in amber)
//!   ✻ ⊘      claude-ps could not be run         (⊘ in red)
//! ```
//!
//! The mark never changes — it is identity, not state. Everything that varies is the badge
//! beside it, and its colour is what the old `◇`/`◆`/`◈` glyph pair used to carry.
//!
//! One click down, the menu draws each session with `luneta`'s glyph for its status —
//! 🙋 `waiting`, ☕ `idle`, 🐚 `shell`, 🛸 anything else, and a braille spinner that actually
//! turns for `busy` — beside the status word itself, in `luneta`'s order: attention first, most
//! recent first within a status. 🔴 **All of that is `luneta`'s and this end does not get a
//! vote**: the same agent read on two surfaces has to be the same picture in the same place.
//! Only the spinner's *frames* are this end's own — heavier ones, because a menu row is not a
//! terminal line and the picker's three-dot cells vanish into the theme foreground; see
//! `state::SPINNER`.
//!
//! ⚠️ The one judgment left on top of that is **what the badge counts**, and it is `waiting`
//! alone: an agent with a question pending. `idle` sorts above the working rows and is not
//! counted, because nothing retires it — see [`state::State::is_actionable`].
//!
//! # Shape
//!
//! - [`agents`] shells out to `claude-ps` and deserialises the JSON array it prints. 🔴 It does
//!   **not** read `~/.claude/sessions` — one liveness implementation, several consumers, so the
//!   applet and `luneta` cannot disagree about what is alive.
//! - [`state`] owns the whole mapping from raw status to what is shown, because the thing that
//!   draws the badge and the thing that builds the menu must be the same thing.
//! - [`mark`] rasterises the Claude mark from the official SVG, at whatever height the bar
//!   asks for. [`icon`] composes it with the badge. [`jump`] focuses a pane. [`tray`] is the
//!   SNI item.
//!
//! # Running
//!
//! Needs `claude-ps` on `PATH` and a session bus. Waybar needs no configuration change: a
//! `tray` module is enough, and this arrives in it like blueman or Telegram.

mod agents;
mod icon;
mod jump;
mod mark;
mod state;
mod tray;

use ksni::blocking::TrayMethods;
use std::sync::Arc;
use std::time::Duration;

/// The animation tick — ten a second, which is what the busy spinner needs to read as motion
/// rather than as a glyph that keeps changing its mind. `luneta`'s rate, so the two spin at
/// the same speed even where `state::SPINNER` gives this end heavier frames to spin.
///
/// ⚠️ This is **not** the poll interval, and the two were the same number until the spinner
/// arrived. See [`TICKS_PER_POLL`]: speeding this up without that divisor would quietly have
/// taken `claude-ps` from one process every five seconds to ten every second.
const TICK: Duration = Duration::from_millis(100);

/// Animation ticks per poll, so the producer still runs once every five seconds.
///
/// Cheap — a process spawn and a few small reads — but not free, and nothing on screen changes
/// faster than a human notices. The menu additionally re-polls on the way to opening, so this
/// interval only governs how quickly the *badge* catches up.
///
/// ⚠️ The age column does not wait on it. A rendered age is the snapshot's plus how long ago the
/// snapshot was taken, so it advances on every repaint rather than in five-second steps — see
/// [`state::Entry::label`].
const TICKS_PER_POLL: u64 = 50;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let renderer = match icon::Renderer::load() {
        Ok(r) => r,
        // Nothing can be drawn, so there is nothing to put in the tray. Fail here, loudly, and
        // let the service supervisor say so — rather than publishing an invisible item.
        Err(e) => {
            eprintln!("claude-tray: {e}");
            std::process::exit(1);
        }
    };

    // 🔴 Waybar is a Hyprland `exec-once`, so it claims the watcher *after* the systemd user
    // manager has already started us. Without this the first `spawn()` loses that race every
    // login and exits 1. `assume_sni_available` turns "no tray host yet" from an exit into a
    // wait: ksni keeps the item and registers it when the host appears, and again after every
    // Waybar restart. The cost is that a box with genuinely no tray support fails silently
    // instead of loudly, which is why `watcher_offline` leaves a line in the journal.
    let anim = Arc::new(tray::Animation::default());
    let handle = tray::ClaudeTray::new(renderer, Arc::clone(&anim))
        .assume_sni_available(true)
        .spawn()?;

    let mut ticks: u64 = 0;
    loop {
        std::thread::sleep(TICK);
        ticks += 1;
        // Always, so the spinner's phase follows the wall clock rather than however long the
        // menu happened to be open — a menu reopened a second later picks the cycle up where it
        // would have been, not where it was left.
        anim.advance();

        // 🔴 Two different reasons to repaint, and only one of them asks the producer anything.
        // A tick that is neither is spent by saying so: `Handle::update` rebuilds the whole menu
        // and re-renders the pixmap just to diff them, so calling it on every tick would burn
        // that ten times a second to put back pixels nobody is looking at.
        let poll = ticks.is_multiple_of(TICKS_PER_POLL);
        if !poll && !anim.is_spinning() {
            continue;
        }

        if handle
            .update(|t: &mut tray::ClaudeTray| {
                if poll {
                    t.refresh();
                }
            })
            .is_none()
        {
            // The tray service is gone; there is no bar to update any more. This is a
            // failure, not a clean finish — exit non-zero so the supervisor brings us back
            // rather than leaving the bar permanently empty.
            eprintln!("claude-tray: tray service stopped unexpectedly");
            std::process::exit(1);
        }
    }
}
