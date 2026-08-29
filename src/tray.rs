//! The StatusNotifierItem itself: what Waybar reads, and what the menu says.

use crate::icon::{BLOCKED, COUNT, FAULT, Renderer};
use crate::state::{Entry, Snapshot, State, snapshot};
use crate::{agents, jump};
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, Status};

/// What the last poll found. An error is a state the tray *shows*, not a reason to exit —
/// `claude-agents` missing from `PATH` is the one failure this slice can really have, and a
/// silent tray would hide exactly the thing the applet exists to make visible.
enum View {
    Agents(Snapshot),
    Broken(String),
}

pub struct ClaudeTray {
    renderer: Renderer,
    view: View,
}

impl ClaudeTray {
    pub fn new(renderer: Renderer) -> Self {
        let mut tray = Self {
            renderer,
            view: View::Broken("not polled yet".into()),
        };
        tray.refresh();
        tray
    }

    /// Ask the producer, classify, and keep the result.
    pub fn refresh(&mut self) {
        self.view = match agents::poll() {
            Ok(rows) => View::Agents(snapshot(&rows, now())),
            Err(e) => View::Broken(e.to_string()),
        };
    }

    /// What goes beside the mark, and in what colour. 🔴 **The mark itself is never part of
    /// this** — it is identity, and it stays [`crate::mark::CLAUDE`] in every state. Three things can
    /// appear here and each has exactly one meaning:
    ///
    /// - nothing, in the calm case — the mark alone;
    /// - the count in [`COUNT`], the bar's own foreground, when turns have merely finished;
    /// - the count in [`BLOCKED`] amber when at least one session is *stuck on him*, which is
    ///   what the retired `◈` glyph used to say;
    /// - `⊘` in [`FAULT`] red — not "you have work" but *the applet cannot see*. `⊘` was
    ///   originally *unreachable*, which cannot happen on one box; a producer that is missing
    ///   or exiting non-zero is the real failure, and it inherits the shape.
    fn badge(&self) -> (String, [u8; 3]) {
        match &self.view {
            View::Broken(_) => ("\u{2298}".to_string(), FAULT),
            View::Agents(s) if s.blocked => (s.badge_text(), BLOCKED),
            View::Agents(s) => (s.badge_text(), COUNT),
        }
    }
}

/// Unix seconds. A clock that cannot be read is treated as the epoch, which makes every session
/// look ancient — dormant, uncounted, still listed. Wrong, but wrong in the quiet direction.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One menu row. Clicking it puts him in front of that session — see [`crate::jump`], which
/// switches the terminal he already has to that session, or opens one when there is none.
///
/// 🔴 **A row is enabled exactly when it has somewhere to send him**, and dormant rows have
/// somewhere just like the rest. [[CSB-11]] made `enabled: false` the way to *look* secondary,
/// and [[CSB-12]] hung it on `Dormant` — reasonable when the click was a no-op, and wrong the
/// moment [[CSB-17]] made a click actually land somewhere. It meant the rows he is most likely
/// to have **forgotten about** were the only ones he could not jump to, which inverts
/// [[CSB-3]]'s whole reason for listing them: ageing out means *stops nagging*, not *is lost*.
/// Verified against the real menu — GTK reports a dormant row `sensitive=false`, so the grey was
/// not merely cosmetic and the click genuinely could not be made.
///
/// *Uncounted* is already said twice over, by the `·` glyph and by the row's absence from the
/// badge. It does not need saying a third time in a flag that also blocks the jump. So the only
/// dimmed rows left are agents outside zellij, which have no address to send him to — where
/// dimmed means **inert**, which is what it should have meant all along.
fn row(entry: &Entry) -> MenuItem<ClaudeTray> {
    let target = entry.target.clone();
    StandardItem {
        label: entry.label(),
        enabled: target.is_some(),
        activate: Box::new(move |_: &mut ClaudeTray| {
            if let Some(t) = &target
                && let Err(e) = jump::focus(t)
            {
                eprintln!("claude-tray: {e}");
            }
        }),
        ..Default::default()
    }
    .into()
}

fn note(text: impl Into<String>) -> MenuItem<ClaudeTray> {
    StandardItem {
        label: text.into(),
        enabled: false,
        ..Default::default()
    }
    .into()
}

impl ksni::Tray for ClaudeTray {
    /// 🔴 Left click opens the menu instead of firing `Activate`. The whole reason this is a
    /// real tray item rather than a Waybar `custom/*` module was Lorenzo's "like the telegram
    /// and bluetooth ones, that I can click and show details".
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "claude-tray".into()
    }

    /// 🔴 **Must stay empty.** Waybar's `getIconPixbuf` returns the *named* icon whenever
    /// `IconName` is non-empty and only then falls back to `IconPixmap`, so a stray name here
    /// silently discards everything [`crate::icon`] draws.
    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let (badge, rgb) = self.badge();
        vec![self.renderer.render(&badge, rgb)]
    }

    fn title(&self) -> String {
        match &self.view {
            View::Agents(s) if s.badge == 0 => "claude — nothing waiting".into(),
            View::Agents(s) => format!("claude — {} waiting", s.badge),
            View::Broken(e) => format!("claude — {e}"),
        }
    }

    /// 🔴 **Never `Passive`.** Waybar's `show-passive-items` defaults to false and it hides
    /// passive items outright, so a calm applet marked passive would *vanish* rather than sit
    /// there showing the bare mark — the exact failure this whole effort exists to prevent.
    ///
    /// `NeedsAttention` cannot carry its own pixmap (`AttentionIconPixmap` is an unimplemented
    /// TODO in Waybar), but `Item::setStatus` does add a `needs-attention` CSS class — and a
    /// tray item being a `Gtk::Image`, the only thing that class can actually do is draw a
    /// border. So this is the *second* cue, under the badge: the pixmap says what and how many,
    /// the border says look now.
    fn status(&self) -> Status {
        match &self.view {
            View::Agents(s) => {
                if s.badge > 0 {
                    Status::NeedsAttention
                } else {
                    Status::Active
                }
            }
            // ⚠️ A broken producer is *also* attention. It does not break the
            // `badge > 0 ⟺ something to do` invariant, because in this state there is no badge
            // at all — `⊘` renders instead of a number, so the colour cannot be misread as a
            // count. What it would otherwise be is an applet sitting there looking calm while
            // it is in fact blind, which is the nightmare this whole effort exists to prevent.
            View::Broken(_) => Status::NeedsAttention,
        }
    }

    /// Poll once more on the way to opening, so the list is never a poll interval stale at the
    /// moment it is actually read.
    fn menu_about_to_show(&mut self) {
        self.refresh();
    }

    /// The applet outlives any particular tray host: systemd starts it at login, Waybar claims
    /// the watcher later, and a Waybar restart drops and reclaims it. ksni handles the
    /// re-registration; these two exist so the journal can tell the two invisible states apart —
    /// *waiting for a bar* and *actually broken* look identical in the tray, which is precisely
    /// the confusion this applet was built to end.
    fn watcher_online(&self) {
        eprintln!("claude-tray: tray host appeared, item registered");
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        eprintln!("claude-tray: no tray host ({reason:?}), waiting for one");
        // Keep the service alive and keep polling; the item re-registers when a host returns.
        true
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let snap = match &self.view {
            View::Broken(e) => {
                return vec![note(format!("\u{2298}  {e}")), MenuItem::Separator, quit()];
            }
            View::Agents(s) => s,
        };

        if snap.entries.is_empty() {
            return vec![note("no agents running"), MenuItem::Separator, quit()];
        }

        let mut items: Vec<MenuItem<Self>> = Vec::new();

        // Actionable first — the reason the applet is in the bar at all.
        for state in [State::NeedsInput, State::YourTurn] {
            items.extend(snap.iter(state).map(row));
        }

        // Then what is merely running. A divider, because "wants you" and "is busy" are
        // different questions and the eye should not have to read the state column to tell.
        let working: Vec<_> = snap.iter(State::Working).map(row).collect();
        if !working.is_empty() {
            if !items.is_empty() {
                items.push(MenuItem::Separator);
            }
            items.extend(working);
        }

        // 🔴 And then what has aged out: **listed, dimmed, uncounted — not hidden.** An hour is
        // only a safe threshold because crossing it means *stops nagging*, not *is lost*.
        let dormant: Vec<_> = snap.iter(State::Dormant).map(row).collect();
        if !dormant.is_empty() {
            if !items.is_empty() {
                items.push(MenuItem::Separator);
            }
            items.extend(dormant);
        }

        items.push(MenuItem::Separator);
        items.push(quit());
        items
    }
}

fn quit() -> MenuItem<ClaudeTray> {
    StandardItem {
        label: "Quit".into(),
        activate: Box::new(|_: &mut ClaudeTray| std::process::exit(0)),
        ..Default::default()
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Target;

    fn entry(state: State, target: Option<Target>) -> Entry {
        Entry {
            state,
            title: "infra".into(),
            age_s: 60,
            target,
        }
    }

    fn somewhere() -> Option<Target> {
        Some(Target {
            session: "infra".into(),
            pane: "0".into(),
        })
    }

    fn enabled(item: &MenuItem<ClaudeTray>) -> bool {
        match item {
            MenuItem::Standard(s) => s.enabled,
            _ => panic!("not a standard item"),
        }
    }

    /// 🔴 [[CSB-18]]. A dormant row is the one he has most likely forgotten, so it is the last
    /// row that should refuse to take him there. GTK marks `enabled: false` rows genuinely
    /// insensitive, so this flag is the jump, not a shade of grey.
    #[test]
    fn a_dormant_row_can_still_be_jumped_to() {
        assert!(enabled(&row(&entry(State::Dormant, somewhere()))));
    }

    #[test]
    fn every_state_with_an_address_is_reachable() {
        for state in [
            State::NeedsInput,
            State::YourTurn,
            State::Working,
            State::Dormant,
        ] {
            assert!(enabled(&row(&entry(state, somewhere()))), "{state:?}");
        }
    }

    /// ⚠️ The one thing dimming still means: an agent outside zellij has no address, so the row
    /// is readable and inert rather than a click that quietly does nothing.
    #[test]
    fn a_row_with_nowhere_to_go_is_inert() {
        assert!(!enabled(&row(&entry(State::Working, None))));
    }
}
