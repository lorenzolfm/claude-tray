//! The StatusNotifierItem itself: what Waybar reads, and what the menu says.

use crate::icon::{BLOCKED, FAULT, Renderer};
use crate::state::{Entry, Snapshot, snapshot};
use crate::{agents, jump};
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, Status};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// How long the spinner continues to turn after a menu opens.
///
/// This value is an estimate, because there is no signal that the menu closed.
/// `com.canonical.dbusmenu` reports an open (`AboutToShow`, which reaches
/// [`ksni::Tray::menu_about_to_show`]). But ksni 0.3.6 sends only `clicked` out of `Event`, so
/// the `closed` signal from the host does not arrive. This time limit replaces it, and it is
/// wrong in two directions:
///
/// - if the menu closes before the limit, the applet continues to tick for the remainder. That
///   sends ten small `ItemsPropertiesUpdated` a second to nobody. No producer runs, so the cost
///   is signals and not processes.
/// - if the menu stays open past the limit, the spinners stop, which looks like a stuck applet
///   and not a stuck agent.
///
/// One minute accepts the second failure and prevents the first, because a person does not look
/// at a tray menu for one minute but does close one after two seconds.
const SPIN_AFTER_OPEN: u64 = 60_000;

/// The animation clock. The poll loop advances it, and the tray reads it.
///
/// The two share it so that a quiet tick stays cheap. `Handle::update` computes each tray
/// property again and rebuilds the full menu to compare them, so a question to the tray about a
/// repaint costs as much as the repaint. These three cells let `main` decide not to call it at
/// all, which is the difference between a busy machine and an idle one at each tick with the
/// menu closed.
#[derive(Debug, Default)]
pub struct Animation {
    /// Ticks since the start. The value only increases, so each busy row in the menu turns from
    /// one clock and the rows move together.
    frame: AtomicU64,
    /// Unix milliseconds at the last `AboutToShow`, or 0 if the menu never opened.
    opened_at_ms: AtomicU64,
    /// Did the last poll find a row with a glyph that moves? [`ClaudeTray::refresh`] writes it,
    /// so it is one poll old at worst. An old `true` costs one repaint and not a wrong
    /// picture.
    spinning: AtomicBool,
}

impl Animation {
    /// One tick. The cost is low, so this runs at each tick. The phase of the spinner is thus a
    /// function of the clock and not of the time that the menu was open.
    pub fn advance(&self) {
        self.frame.fetch_add(1, Ordering::Relaxed);
    }

    pub fn frame(&self) -> u64 {
        self.frame.load(Ordering::Relaxed)
    }

    /// Does something on screen turn now, and is this tick thus worth a repaint?
    ///
    /// Both conditions apply, and the second one is the important one: a busy agent that nobody
    /// looks at is not a reason for a repaint.
    pub fn is_spinning(&self) -> bool {
        self.spinning.load(Ordering::Relaxed)
            && now_ms().saturating_sub(self.opened_at_ms.load(Ordering::Relaxed)) < SPIN_AFTER_OPEN
    }
}

/// What the last poll found. An error is a state that the tray shows and not a reason to exit.
/// An absent `claude-ps` on `PATH` is the most probable failure, and a tray that stayed quiet
/// would hide the state that the applet exists to show.
enum View {
    Agents(Snapshot),
    Broken(String),
}

pub struct ClaudeTray {
    renderer: Renderer,
    view: View,
    anim: Arc<Animation>,
}

impl ClaudeTray {
    pub fn new(renderer: Renderer, anim: Arc<Animation>) -> Self {
        let mut tray = Self {
            renderer,
            view: View::Broken("not polled yet".into()),
            anim,
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
        // Write the value instead of a read by the loop, for the reason in [`Animation`]: the
        // loop cannot read it without a repaint. A failed producer has no rows and thus no
        // spinner.
        self.anim.spinning.store(
            match &self.view {
                View::Agents(s) => s.any_spinning(),
                View::Broken(_) => false,
            },
            Ordering::Relaxed,
        );
    }

    /// What goes beside the mark, and in which colour. The mark is not part of this. It is
    /// identity, and it stays [`crate::mark::CLAUDE`] in each state. Three values can appear
    /// here, and each one has one meaning:
    ///
    /// - nothing, in the quiet state, which shows the mark alone;
    /// - the count in [`BLOCKED`] amber, which means that an agent waits for you;
    /// - `⊘` in [`FAULT`] red, which does not mean "you have work" but that the applet cannot
    ///   see. A producer that is absent or that exits non-zero is the failure that it reports.
    fn badge(&self) -> (String, [u8; 3]) {
        match &self.view {
            View::Broken(_) => ("\u{2298}".to_string(), FAULT),
            View::Agents(s) => (s.badge_text(), BLOCKED),
        }
    }
}

/// Unix seconds. If the clock cannot be read, this gives the epoch, which stops the ages
/// between polls. `Snapshot::since` saturates, so a row then shows the age from the producer
/// instead of a wrong age.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix milliseconds, for [`SPIN_AFTER_OPEN`]. This uses the same clock as [`now`] and not an
/// `Instant`, because the two sides of [`Animation`] are on different threads and an `Instant`
/// is not a number that an atomic can hold. A clock that cannot be read gives the epoch here
/// too, which puts each open outside the time limit and stops the spinner.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One menu row. A click on it moves you to that session. See [`crate::jump`], which changes
/// the session in the terminal that you have, or opens a terminal if there is none.
///
/// A row is enabled when it has a destination, in each state. An earlier design used
/// `enabled: false` to make a row look secondary, and it applied that to the oldest rows. GTK
/// gives such a row `sensitive=false`, so the grey was not only a colour and the click could not
/// occur. The rows that you most probably forgot were thus the only rows that you could not
/// jump to.
///
/// The rule from that failure stays: a grey row does not mean uncounted, which the glyph and the
/// badge already show. The only grey rows are agents outside zellij, which have no address. Grey
/// thus means that the row does nothing.
fn row(entry: &Entry, frame: u64, since: u64) -> MenuItem<ClaudeTray> {
    let target = entry.target.clone();
    StandardItem {
        label: entry.label(frame, since),
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
    /// A left click opens the menu instead of an `Activate` event. This item exists as a real
    /// tray item, and not as a Waybar `custom/*` module, so that a click shows the details in
    /// the same way as the Telegram and Bluetooth items.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "claude-tray".into()
    }

    /// This must stay empty. Waybar's `getIconPixbuf` returns the named icon while `IconName`
    /// has a value, and it uses `IconPixmap` only if that name is empty. A name here thus
    /// removes each pixel that [`crate::icon`] draws.
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

    /// Never use `Passive`. Waybar's `show-passive-items` is false by default, and Waybar hides
    /// a passive item. A quiet applet with that status would thus disappear instead of a display
    /// of the mark alone, which is the failure that this program prevents.
    ///
    /// `NeedsAttention` cannot carry its own pixmap, because `AttentionIconPixmap` is not
    /// implemented in Waybar. But `Item::setStatus` adds a `needs-attention` CSS class, and a
    /// tray item is a `Gtk::Image`, so that class can only draw a border. The border is thus the
    /// second signal, below the badge: the pixmap gives the number, and the border says to look
    /// now.
    fn status(&self) -> Status {
        match &self.view {
            View::Agents(s) => {
                if s.badge > 0 {
                    Status::NeedsAttention
                } else {
                    Status::Active
                }
            }
            // A failed producer also needs attention. It keeps the rule that `badge > 0` means
            // that there is work, because this state has no badge: the applet draws `⊘` instead
            // of a number, so nobody can read the colour as a count. Without this, the applet
            // would look quiet while it cannot see.
            View::Broken(_) => Status::NeedsAttention,
        }
    }

    /// Poll again as the menu opens, so that the list is not one poll interval old when a
    /// person reads it.
    ///
    /// Record the time too. This is the only signal that tells the applet that a person looks at
    /// the menu, and it starts the spinner. See [`SPIN_AFTER_OPEN`] for the signal that does not
    /// arrive.
    fn menu_about_to_show(&mut self) {
        self.anim.opened_at_ms.store(now_ms(), Ordering::Relaxed);
        self.refresh();
    }

    /// The applet continues after a tray host stops. systemd starts it at login, Waybar claims
    /// the watcher later, and a restart of Waybar releases and claims it again. ksni registers
    /// the item again. These two functions write to the journal, so that a reader can separate
    /// the two invisible states: a wait for a bar, and a failure.
    fn watcher_online(&self) {
        eprintln!("claude-tray: tray host appeared, item registered");
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        eprintln!("claude-tray: no tray host ({reason:?}), waiting for one");
        // Keep the service and the polls. The item registers again when a host returns.
        true
    }

    /// One flat list, in `luneta`'s order. There are no dividers between groups of rows,
    /// because the glyph and the word already show the group and because a menu with different
    /// groups from the picker's list would disagree with it. The one separator is above `Quit`,
    /// and it marks the end of the list and not a group.
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let frame = self.anim.frame();
        let snap = match &self.view {
            View::Broken(e) => {
                return vec![note(format!("\u{2298}  {e}")), MenuItem::Separator, quit()];
            }
            View::Agents(s) => s,
        };

        if snap.entries.is_empty() {
            return vec![note("no agents running"), MenuItem::Separator, quit()];
        }

        // Read this one time for the full menu and not for each row. The offset must be the
        // same number in each row, or a slow rebuild shows two different times.
        let since = snap.since(now());

        let mut items: Vec<MenuItem<Self>> =
            snap.entries.iter().map(|e| row(e, frame, since)).collect();

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
    use crate::state::{Status, Target};

    /// A row is built from a status word, as the real one is. There is no way to ask for a row
    /// that ranks as one status and reads as another.
    fn entry(status: &str, target: Option<Target>) -> Entry {
        Entry {
            status: Status::parse(status.into()),
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

    fn label(item: &MenuItem<ClaudeTray>) -> String {
        match item {
            MenuItem::Standard(s) => s.label.clone(),
            _ => panic!("not a standard item"),
        }
    }

    /// The row that you most probably forgot must not refuse the jump. GTK makes an
    /// `enabled: false` row insensitive, so this flag controls the jump and not only a colour.
    #[test]
    fn every_state_with_an_address_is_reachable() {
        // One word for each of the four ranks. `shell` is the fourth, because a status that
        // this build does not know still gets a row.
        for status in ["waiting", "idle", "busy", "shell"] {
            assert!(enabled(&row(&entry(status, somewhere()), 0, 0)), "{status}");
        }
    }

    /// The one meaning that grey keeps: an agent outside zellij has no address, so the row is
    /// legible and does nothing, instead of a click with no result.
    #[test]
    fn a_row_with_nowhere_to_go_is_inert() {
        assert!(!enabled(&row(&entry("busy", None), 0, 0)));
    }

    /// The menu rebuilds ten times a second from one snapshot, so the offset must reach the
    /// label or the age column stops with the list.
    #[test]
    fn a_rows_age_counts_on_while_the_menu_is_open() {
        let fresh = label(&row(&entry("idle", somewhere()), 0, 0));
        let later = label(&row(&entry("idle", somewhere()), 0, 120));
        assert!(fresh.ends_with("idle 1m"), "{fresh}");
        assert!(later.ends_with("idle 3m"), "{later}");
    }
}
