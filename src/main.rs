//! **claude-tray** — which Claude Code sessions wait for you, in the system tray.
//!
//! Many concurrent Claude Code sessions run inside zellij, and there is no way to see which of
//! them need input or have finished. This program publishes a StatusNotifierItem, so that state
//! is always visible: the Claude mark and a count stay in the bar, and the session list is one
//! click away.
//!
//! ```text
//!   ✻        nothing waits for you                (the mark, alone)
//!   ✻ 2      two agents have asked a question     (count in amber)
//!   ✻ ⊘      claude-ps could not run              (⊘ in red)
//! ```
//!
//! The mark does not change, because it is identity and not state. Only the badge beside it
//! changes, and its colour carries what the older `◇`/`◆`/`◈` glyphs carried.
//!
//! The menu shows each session with `luneta`'s glyph for its status: 🙋 `waiting`, ☕ `idle`,
//! 🐚 `shell`, 🛸 for any other value, and a braille spinner that turns for `busy`. The glyph
//! goes beside the status word itself, in `luneta`'s order: attention first, and most recent
//! first within one status. All of that comes from `luneta`, because the same agent read on two
//! surfaces must give the same picture in the same place. Only the frames of the spinner belong
//! to this program. They are heavier, because a menu row is not a terminal line and the picker's
//! three-dot cells disappear into the theme foreground. See `state::SPINNER`.
//!
//! The one decision on top of that is what the badge counts, and it counts `waiting` alone: an
//! agent with a question pending. `idle` sorts above the working rows, but nothing ends that
//! state, so the badge does not count it. See [`state::State::is_actionable`].
//!
//! # Shape
//!
//! - [`agents`] runs `claude-ps` and deserialises the JSON array that it prints. It does not
//!   read `~/.claude/sessions`, so that one liveness implementation serves several consumers and
//!   the applet and `luneta` cannot disagree about what is alive.
//! - [`state`] holds the full mapping from raw status to what the applet shows, because the code
//!   that draws the badge and the code that builds the menu must be the same code.
//! - [`mark`] rasterises the Claude mark from the official SVG, at the height that the bar asks
//!   for. [`icon`] composes it with the badge. [`jump`] focuses a pane. [`tray`] is the SNI item.
//!
//! # Running
//!
//! The program needs `claude-ps` on `PATH` and a session bus. Waybar needs no change of
//! configuration: a `tray` module is sufficient, and this item appears in it like blueman or
//! Telegram.

mod agents;
mod icon;
mod jump;
mod mark;
mod state;
mod tray;

use ksni::blocking::TrayMethods;
use std::sync::Arc;
use std::time::Duration;

/// The animation tick: ten a second, which the busy spinner needs to look like motion. This is
/// `luneta`'s rate, so the two spinners turn at the same speed.
///
/// This is not the poll interval. See [`TICKS_PER_POLL`]: without that divisor, a shorter tick
/// would take `claude-ps` from one process every five seconds to ten each second.
const TICK: Duration = Duration::from_millis(100);

/// Animation ticks per poll, so that the producer still runs once every five seconds.
///
/// A poll is cheap but not free: one process and a few small reads. Nothing on screen changes
/// faster than a person can see. The menu also polls again as it opens, so this interval only
/// controls how quickly the badge catches up.
///
/// The age column does not wait for it. A shown age is the age in the snapshot plus the time
/// since the snapshot, so it increases at each repaint. See [`state::Entry::label`].
const TICKS_PER_POLL: u64 = 50;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let renderer = match icon::Renderer::load() {
        Ok(r) => r,
        // Nothing can be drawn, so there is nothing to put in the tray. Fail here and let the
        // service supervisor report it, instead of publishing an invisible item.
        Err(e) => {
            eprintln!("claude-tray: {e}");
            std::process::exit(1);
        }
    };

    // Waybar is a Hyprland `exec-once`, so it claims the watcher after the systemd user manager
    // has started this program. Without the flag below, the first `spawn()` loses that race at
    // each login and exits 1. `assume_sni_available` makes "no tray host yet" a wait instead of
    // an exit: ksni keeps the item and registers it when a host appears, and again after each
    // restart of Waybar. The cost is that a machine with no tray support fails silently, which
    // is why `watcher_offline` writes a line to the journal.
    let anim = Arc::new(tray::Animation::default());
    let handle = tray::ClaudeTray::new(renderer, Arc::clone(&anim))
        .assume_sni_available(true)
        .spawn()?;

    let mut ticks: u64 = 0;
    loop {
        std::thread::sleep(TICK);
        ticks += 1;
        // Always advance, so that the phase of the spinner follows the clock and not the time
        // that the menu was open. A menu that opens again one second later continues the cycle
        // from where the clock puts it.
        anim.advance();

        // There are two reasons to repaint, and only one of them calls the producer. A tick
        // with neither reason ends here. `Handle::update` rebuilds the full menu and renders the
        // pixmap again only to compare them, so a call at each tick would do that work ten times
        // a second for pixels that nobody looks at.
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
            // The tray service has gone, so there is no bar to update. This is a failure and
            // not a clean end. Exit non-zero, so that the supervisor starts the program again
            // instead of leaving the bar empty.
            eprintln!("claude-tray: tray service stopped unexpectedly");
            std::process::exit(1);
        }
    }
}
