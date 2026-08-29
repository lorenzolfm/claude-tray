//! **claude-tray** — which Claude Code sessions are waiting on you, in the system tray.
//!
//! Lorenzo runs many concurrent Claude Code sessions inside zellij and cannot tell which ones
//! need input or have finished. This publishes a StatusNotifierItem so that state is ambient:
//! a glyph and a count always in the bar, the session list one click away.
//!
//! ```text
//!   ◇        nothing wants you
//!   ◆ 3      three finished their turn
//!   ◈ 2      at least one is blocked on you
//!   ⊘        claude-agents could not be run
//! ```
//!
//! # Shape
//!
//! - [`agents`] shells out to `claude-agents` and parses nine TAB-separated columns. 🔴 It does
//!   **not** read `~/.claude/sessions` — one liveness implementation, several consumers, so the
//!   applet and `zj-picker` cannot disagree about what is alive.
//! - [`state`] owns the whole mapping from raw status to what is shown, because the thing that
//!   draws the badge and the thing that builds the menu must be the same thing.
//! - [`icon`] rasterises the badge. [`jump`] focuses a pane. [`tray`] is the SNI item.
//!
//! # Running
//!
//! Needs `claude-agents` on `PATH` and a session bus. Waybar needs no configuration change: a
//! `tray` module is enough, and this arrives in it like blueman or Telegram.

mod agents;
mod icon;
mod jump;
mod state;
mod tray;

use ksni::blocking::TrayMethods;
use std::time::Duration;

/// Cheap — a process spawn and a few small reads — but not free, and nothing on screen changes
/// faster than a human notices. The menu additionally re-polls on the way to opening, so this
/// interval only governs how quickly the *badge* catches up.
const POLL: Duration = Duration::from_secs(5);

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

    let handle = tray::ClaudeTray::new(renderer).spawn()?;

    loop {
        std::thread::sleep(POLL);
        if handle
            .update(|t: &mut tray::ClaudeTray| t.refresh())
            .is_none()
        {
            // The tray service is gone; there is no bar to update any more.
            return Ok(());
        }
    }
}
