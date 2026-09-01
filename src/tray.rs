use crate::icon::Renderer;
use crate::state::{Badge, Entry, Snapshot, snapshot};
use crate::{agents, jump};
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, Status};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const SPIN_AFTER_OPEN: u64 = 60_000;

#[derive(Debug, Default)]
pub struct Animation {
    frame: AtomicU64,
    opened_at_ms: AtomicU64,
    spinning: AtomicBool,
}

impl Animation {
    pub fn advance(&self) {
        self.frame.fetch_add(1, Ordering::Relaxed);
    }

    pub fn frame(&self) -> u64 {
        self.frame.load(Ordering::Relaxed)
    }

    pub fn is_spinning(&self) -> bool {
        self.spinning.load(Ordering::Relaxed)
            && opened_recently(now_ms(), self.opened_at_ms.load(Ordering::Relaxed))
    }
}

enum View {
    Agents(Snapshot),
    Broken(agents::Error),
}

impl View {
    fn spins(&self) -> bool {
        match self {
            View::Agents(s) => s.any_spinning(),
            View::Broken(_) => false,
        }
    }
}

fn look() -> View {
    match agents::poll() {
        Ok(rows) => View::Agents(snapshot(&rows, now())),
        Err(e) => View::Broken(e),
    }
}

pub struct ClaudeTray {
    renderer: Renderer,
    view: View,
    anim: Arc<Animation>,
}

impl ClaudeTray {
    pub fn new(renderer: Renderer, anim: Arc<Animation>) -> Self {
        let view = look();
        anim.spinning.store(view.spins(), Ordering::Relaxed);
        Self {
            renderer,
            view,
            anim,
        }
    }

    pub fn refresh(&mut self) {
        self.view = look();
        self.anim
            .spinning
            .store(self.view.spins(), Ordering::Relaxed);
    }

    fn badge(&self) -> Badge {
        match &self.view {
            View::Broken(_) => Badge::Blind,
            View::Agents(s) => s.badge(),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .ok()
}

fn opened_recently(now: Option<u64>, opened_at_ms: u64) -> bool {
    match now.and_then(|now| now.checked_sub(opened_at_ms)) {
        Some(open_for) => open_for < SPIN_AFTER_OPEN,
        None => false,
    }
}

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
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "claude-tray".into()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let badge = self.badge();
        vec![self.renderer.render(&badge.text(), badge.rgb())]
    }

    fn title(&self) -> String {
        match &self.view {
            View::Broken(e) => format!("claude — {e}"),
            View::Agents(s) => match s.badge() {
                Badge::Waiting(n) => format!("claude — {n} waiting"),
                Badge::Quiet | Badge::Blind => "claude — nothing waiting".into(),
            },
        }
    }

    fn status(&self) -> Status {
        if self.badge().needs_attention() {
            Status::NeedsAttention
        } else {
            Status::Active
        }
    }

    fn menu_about_to_show(&mut self) {
        self.anim
            .opened_at_ms
            .store(now_ms().unwrap_or(0), Ordering::Relaxed);
        self.refresh();
    }

    fn watcher_online(&self) {
        eprintln!("claude-tray: tray host appeared, item registered");
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        eprintln!("claude-tray: no tray host ({reason:?}), waiting for one");
        true
    }

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
    use crate::state::{Status as AgentStatus, Target};

    fn entry(status: &str, target: Option<Target>) -> Entry {
        Entry {
            status: AgentStatus::parse(status.into()),
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

    #[test]
    fn every_state_with_an_address_is_reachable() {
        for status in ["waiting", "idle", "busy", "shell"] {
            assert!(enabled(&row(&entry(status, somewhere()), 0, 0)), "{status}");
        }
    }

    #[test]
    fn a_row_with_nowhere_to_go_is_inert() {
        assert!(!enabled(&row(&entry("busy", None), 0, 0)));
    }

    #[test]
    fn a_rows_age_counts_on_while_the_menu_is_open() {
        let fresh = label(&row(&entry("idle", somewhere()), 0, 0));
        let later = label(&row(&entry("idle", somewhere()), 0, 120));
        assert!(fresh.ends_with("idle 1m"), "{fresh}");
        assert!(later.ends_with("idle 3m"), "{later}");
    }

    #[test]
    fn a_menu_that_never_opened_is_never_recent() {
        assert!(!opened_recently(Some(1_700_000_000_000), 0));
    }

    #[test]
    fn a_clock_that_cannot_be_read_turns_nothing() {
        assert!(!opened_recently(None, 0));
        assert!(!opened_recently(None, 1_700_000_000_000));
    }

    #[test]
    fn only_an_open_inside_the_limit_turns() {
        let now = 1_700_000_000_000;
        assert!(opened_recently(Some(now), now));
        assert!(opened_recently(Some(now), now - SPIN_AFTER_OPEN + 1));
        assert!(!opened_recently(Some(now), now - SPIN_AFTER_OPEN));
    }

    #[test]
    fn a_clock_that_went_backwards_turns_nothing() {
        assert!(!opened_recently(Some(1_000), 1_700_000_000_000));
    }
}
