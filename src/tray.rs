//! The StatusNotifierItem itself: what Waybar reads, and what the menu says.

use crate::icon::Renderer;
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

    /// `⊘` re-scoped. It meant *unreachable* in the two-machine design, which cannot happen on
    /// one box — but a producer that is missing or exiting non-zero is real, and this is the
    /// right shape for it: visibly not the calm `◇`, visibly not a count.
    fn badge_text(&self) -> String {
        match &self.view {
            View::Agents(s) => s.badge_text(),
            View::Broken(_) => "\u{2298}".to_string(),
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

/// One menu row. Clicking it focuses the pane; a row with nowhere to go is inert but still
/// readable, and a dormant row is `enabled: false` — which reads as *dimmed*, i.e. secondary,
/// rather than as broken.
fn row(entry: &Entry) -> MenuItem<ClaudeTray> {
    let target = entry.target.clone();
    StandardItem {
        label: entry.label(),
        enabled: entry.state != State::Dormant,
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
        vec![self.renderer.render(&self.badge_text())]
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
    /// there rendering `◇` — the exact failure this whole effort exists to prevent.
    ///
    /// `NeedsAttention` cannot carry its own pixmap (`AttentionIconPixmap` is an unimplemented
    /// TODO in Waybar), but `Item::setStatus` does add a `needs-attention` CSS class. So this
    /// is how colour reaches `style.css` while the pixmap stays monochrome and themeable.
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
