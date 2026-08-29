//! Clicking a row focuses that agent's zellij pane.
//!
//! The map ruled click-to-jump out of scope, but that was reasoned against building a client on
//! the Mac and reaching `nixos` over SSH. On one local box the reason does not survive: the
//! `pane` column *is* `$ZELLIJ_PANE_ID`, and `zellij action focus-pane-id` takes exactly that,
//! addressed at a session by name from outside it. The whole jump is one process spawn.
//!
//! ⚠️ **What this does not do is raise a window.** Nothing links a Hyprland window to the zellij
//! session running inside it, so the honest scope is: whichever terminal is already attached to
//! that session moves to the right pane. Lorenzo still switches windows himself. Guessing at the
//! window — by title match, or by spawning a second `zellij attach` client — was considered and
//! rejected as guesswork stacked on a clean primitive.

use crate::state::Target;
use std::process::Command;

/// Fire the focus and return what to say if it failed.
///
/// Deliberately not fatal. A tray applet that dies because a session went away between the poll
/// and the click is a worse tool than one that shrugs and repaints.
pub fn focus(target: &Target) -> Result<(), String> {
    let out = Command::new("zellij")
        .args(["--session", &target.session, "action", "focus-pane-id"])
        .arg(&target.pane)
        .output()
        .map_err(|e| format!("zellij: {e}"))?;

    // ⚠️ **The exit code alone is not the answer.** A bad *pane* id exits 2, but a session that
    // has gone away exits **0** and prints the list of live sessions instead. Observed, not
    // assumed. A successful focus is silent on both streams, so silence is the real signal —
    // and it is a stabler one than matching on the text of an error message.
    let noise = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    let (stdout, stderr) = (noise(&out.stdout), noise(&out.stderr));
    if out.status.success() && stdout.is_empty() && stderr.is_empty() {
        return Ok(());
    }

    let why = [&stderr, &stdout]
        .into_iter()
        .filter_map(|s| s.lines().next())
        .find(|l| !l.trim().is_empty())
        .unwrap_or("no such session?")
        .trim();
    Err(format!(
        "focus {}:{} failed: {why}",
        target.session, target.pane
    ))
}
