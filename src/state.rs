use crate::agents::{Row, Zellij};
use crate::icon::{BLOCKED, FAULT};
use std::num::NonZeroUsize;

const NAME_WIDTH: usize = 28;

const SPINNER: [&str; 8] = ["⣾ ", "⣽ ", "⣻ ", "⢿ ", "⡿ ", "⣟ ", "⣯ ", "⣷ "];

const UNKNOWN_GLYPH: &str = "🛸";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Waiting,
    Idle,
    Busy,
    Other,
}

impl State {
    pub fn is_actionable(self) -> bool {
        matches!(self, State::Waiting)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    raw: String,
    state: State,
    face: Face,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    Fixed(&'static str),
    Spinner,
}

impl Status {
    pub fn parse(raw: String) -> Self {
        let (state, face) = match raw.to_ascii_lowercase().as_str() {
            "waiting" => (State::Waiting, Face::Fixed("🙋")),
            "idle" => (State::Idle, Face::Fixed("☕")),
            "busy" => (State::Busy, Face::Spinner),
            "shell" => (State::Other, Face::Fixed("🐚")),
            _ => (State::Other, Face::Fixed(UNKNOWN_GLYPH)),
        };
        Self { raw, state, face }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn glyph(&self, frame: u64) -> &'static str {
        match self.face {
            Face::Fixed(glyph) => glyph,
            Face::Spinner => SPINNER[(frame as usize) % SPINNER.len()],
        }
    }

    pub fn is_spinning(&self) -> bool {
        matches!(self.face, Face::Spinner)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub session: String,
    pub pane: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub status: Status,
    pub title: String,
    pub age_s: u64,
    pub target: Option<Target>,
}

impl Entry {
    pub fn label(&self, frame: u64, since: u64) -> String {
        format!(
            "{}  {:<width$}  {} {}",
            self.status.glyph(frame),
            truncate(&self.title, NAME_WIDTH),
            self.status.as_str(),
            humanise(self.age_s.saturating_add(since)),
            width = NAME_WIDTH,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Quiet,
    Waiting(NonZeroUsize),
    Blind,
}

impl Badge {
    pub fn text(self) -> String {
        match self {
            Badge::Quiet => String::new(),
            Badge::Waiting(n) => n.to_string(),
            Badge::Blind => "\u{2298}".to_string(),
        }
    }

    pub fn rgb(self) -> [u8; 3] {
        match self {
            Badge::Quiet | Badge::Waiting(_) => BLOCKED,
            Badge::Blind => FAULT,
        }
    }

    pub fn needs_attention(self) -> bool {
        !matches!(self, Badge::Quiet)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    pub taken_at: u64,
}

impl Snapshot {
    pub fn badge(&self) -> Badge {
        let waiting = self
            .entries
            .iter()
            .filter(|e| e.status.state().is_actionable())
            .count();
        match NonZeroUsize::new(waiting) {
            Some(n) => Badge::Waiting(n),
            None => Badge::Quiet,
        }
    }

    pub fn any_spinning(&self) -> bool {
        self.entries.iter().any(|entry| entry.status.is_spinning())
    }

    pub fn since(&self, now: u64) -> u64 {
        now.saturating_sub(self.taken_at)
    }
}

pub fn snapshot(rows: &[Row], now: u64) -> Snapshot {
    let mut entries: Vec<Entry> = rows
        .iter()
        .map(|row| Entry {
            status: Status::parse(row.raw_status.clone()),
            title: chosen_name(&row.name, row.name_source.as_deref())
                .or_else(|| row.zellij.as_ref().map(|z| z.session.clone()))
                .unwrap_or_else(|| row.name.clone()),
            age_s: row.transition_age_s,
            target: row
                .zellij
                .as_ref()
                .and_then(Zellij::address)
                .map(|z| Target {
                    session: z.session.clone(),
                    pane: z.pane.clone(),
                }),
        })
        .collect();

    entries.sort_by(|a, b| {
        a.status
            .state()
            .cmp(&b.status.state())
            .then(a.age_s.cmp(&b.age_s))
    });
    name_rows(&mut entries);

    Snapshot {
        entries,
        taken_at: now,
    }
}

fn chosen_name(name: &str, source: Option<&str>) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let chosen = match source {
        None => true,
        Some(source) => source.eq_ignore_ascii_case("user") || source.eq_ignore_ascii_case("peer"),
    };
    chosen.then(|| name.to_string())
}

fn name_rows(entries: &mut [Entry]) {
    let names: Vec<String> = entries.iter().map(|e| e.title.clone()).collect();
    let shared: Vec<bool> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            entry.target.is_some()
                && entries
                    .iter()
                    .enumerate()
                    .any(|(j, other)| j != i && other.target.is_some() && names[j] == names[i])
        })
        .collect();

    for (entry, shared) in entries.iter_mut().zip(shared) {
        if shared && let Some(target) = entry.target.as_ref() {
            entry.title = format!("{}:{}", entry.title, target.pane);
        }
    }
}

fn humanise(secs: u64) -> String {
    match secs {
        0..60 => "<1m".to_string(),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

fn truncate(name: &str, width: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= width {
        return name.to_string();
    }
    let tail = (width - 1) * 5 / 9;
    let head = width - 1 - tail;
    let mut out: String = chars[..head].iter().collect();
    out.push('\u{2026}');
    out.extend(&chars[chars.len() - tail..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn rank(status: &str) -> State {
        Status::parse(status.into()).state()
    }

    fn waiting(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).expect("a waiting badge counts at least one agent")
    }

    fn row(status: &str, transition_age_s: u64) -> Row {
        Row {
            raw_status: status.into(),
            transition_age_s,
            zellij: Some(Zellij {
                session: "s".into(),
                pane: "0".into(),
            }),
            name: "n".into(),
            name_source: Some("derived".into()),
        }
    }

    #[test]
    fn the_states_rank_in_the_pickers_order() {
        assert_eq!(rank("waiting"), State::Waiting);
        assert_eq!(rank("idle"), State::Idle);
        assert_eq!(rank("busy"), State::Busy);
        assert_eq!(rank("shell"), State::Other);

        assert!(State::Waiting < State::Idle);
        assert!(State::Idle < State::Busy);
        assert!(State::Busy < State::Other);
    }

    #[test]
    fn a_status_is_read_case_insensitively() {
        assert_eq!(rank("WAITING"), State::Waiting);
        assert_eq!(rank("Idle"), State::Idle);
    }

    #[test]
    fn an_unknown_status_ranks_last_and_is_never_counted() {
        for unknown in ["shell", "compacting", "", "nonsense"] {
            let s = rank(unknown);
            assert_eq!(s, State::Other, "{unknown:?}");
            assert!(!s.is_actionable(), "{unknown:?}");
        }
    }

    #[test]
    fn an_idle_agent_is_never_counted_at_any_age() {
        for age in [0, 5, 3_600, 7 * 86_400] {
            let snap = snapshot(&[row("idle", age)], NOW);
            assert_eq!(snap.badge(), Badge::Quiet, "{age}s");
            assert_eq!(snap.entries[0].status.state(), State::Idle, "{age}s");
        }
    }

    #[test]
    fn the_badge_counts_exactly_the_waiting_rows() {
        let rows = [
            row("waiting", 10),
            row("idle", 60),
            row("busy", 60),
            row("waiting", 99_999),
            row("shell", 5),
        ];
        let snap = snapshot(&rows, NOW);
        assert_eq!(snap.badge(), Badge::Waiting(waiting(2)));
        assert_eq!(snap.entries.len(), 5, "nothing is hidden, only uncounted");
    }

    #[test]
    fn rows_sort_attention_first_then_most_recent_first() {
        let rows = [
            row("busy", 10),
            row("idle", 300),
            row("waiting", 600),
            row("shell", 1),
            row("idle", 30),
            row("waiting", 60),
        ];
        let states: Vec<(State, u64)> = snapshot(&rows, NOW)
            .entries
            .iter()
            .map(|e| (e.status.state(), e.age_s))
            .collect();
        assert_eq!(
            states,
            vec![
                (State::Waiting, 60),
                (State::Waiting, 600),
                (State::Idle, 30),
                (State::Idle, 300),
                (State::Busy, 10),
                (State::Other, 1),
            ]
        );
    }

    #[test]
    fn equal_ages_keep_the_producers_order() {
        let mut first = row("idle", 42);
        first.zellij = Some(Zellij {
            session: "first".into(),
            pane: "0".into(),
        });
        let mut second = row("idle", 42);
        second.zellij = Some(Zellij {
            session: "second".into(),
            pane: "0".into(),
        });
        let snap = snapshot(&[first, second], NOW);
        assert_eq!(snap.entries[0].title, "first");
        assert_eq!(snap.entries[1].title, "second");
    }

    #[test]
    fn the_age_counts_on_after_the_snapshot_was_taken() {
        let snap = snapshot(&[row("waiting", 59)], NOW);
        assert_eq!(snap.since(NOW), 0);
        assert!(snap.entries[0].label(0, 0).ends_with("<1m"));
        assert_eq!(snap.since(NOW + 90), 90);
        assert!(snap.entries[0].label(0, 90).ends_with("2m"));
    }

    #[test]
    fn a_backwards_clock_reads_as_fresh() {
        assert_eq!(snapshot(&[row("idle", 10)], NOW).since(NOW - 500), 0);
    }

    #[test]
    fn the_row_prints_the_status_it_was_given() {
        let snap = snapshot(&[row("compacting", 120)], NOW);
        let label = snap.entries[0].label(0, 0);
        assert!(label.contains("compacting"), "{label}");
        assert!(label.starts_with(UNKNOWN_GLYPH), "{label}");
    }

    #[test]
    fn a_shell_has_its_own_picture_and_no_rank_of_its_own() {
        let snap = snapshot(&[row("shell", 5)], NOW);
        let entry = &snap.entries[0];
        assert_eq!(entry.status.state(), State::Other);
        assert_eq!(entry.status.glyph(0), "🐚");
        assert_eq!(
            entry.status.glyph(0),
            entry.status.glyph(4),
            "a shell does not turn"
        );
    }

    #[test]
    fn a_derived_name_loses_to_the_zellij_session() {
        assert_eq!(snapshot(&[row("idle", 5)], NOW).entries[0].title, "s");
    }

    #[test]
    fn a_chosen_name_wins_over_the_session() {
        for source in ["user", "peer", "USER"] {
            let mut r = row("idle", 5);
            r.name_source = Some(source.into());
            assert_eq!(snapshot(&[r], NOW).entries[0].title, "n", "{source}");
        }
    }

    #[test]
    fn an_absent_source_is_trusted_and_an_unknown_one_is_not() {
        let mut absent = row("idle", 5);
        absent.name_source = None;
        assert_eq!(snapshot(&[absent], NOW).entries[0].title, "n");

        let mut unknown = row("idle", 5);
        unknown.name_source = Some("something-new".into());
        assert_eq!(snapshot(&[unknown], NOW).entries[0].title, "s");
    }

    #[test]
    fn an_agent_outside_zellij_keeps_the_label_it_has() {
        let mut r = row("idle", 5);
        r.zellij = None;
        let snap = snapshot(&[r], NOW);
        assert_eq!(snap.entries[0].title, "n");
        assert_eq!(snap.entries[0].target, None);
    }

    #[test]
    fn a_session_named_with_whitespace_loses_the_jump_and_keeps_the_name() {
        let mut r = row("idle", 5);
        r.name_source = None;
        r.name = "n".into();
        r.zellij = Some(Zellij {
            session: "my work".into(),
            pane: "0".into(),
        });
        let snap = snapshot(&[r], NOW);
        assert_eq!(snap.entries[0].target, None);
        assert_eq!(snap.entries[0].title, "n");

        let mut derived = row("idle", 5);
        derived.zellij = Some(Zellij {
            session: "my work".into(),
            pane: "0".into(),
        });
        let snap = snapshot(&[derived], NOW);
        assert_eq!(snap.entries[0].target, None);
        assert_eq!(snap.entries[0].title, "my work");
    }

    #[test]
    fn an_empty_session_or_pane_is_no_address() {
        for (session, pane) in [("", "0"), ("s", ""), ("", "")] {
            let mut r = row("idle", 5);
            r.zellij = Some(Zellij {
                session: session.into(),
                pane: pane.into(),
            });
            let snap = snapshot(&[r], NOW);
            assert_eq!(
                snap.entries[0].target, None,
                "session {session:?} pane {pane:?}"
            );
        }
    }

    #[test]
    fn an_ordinary_row_keeps_the_address_it_was_given() {
        let snap = snapshot(&[row("idle", 5)], NOW);
        assert_eq!(
            snap.entries[0].target,
            Some(Target {
                session: "s".into(),
                pane: "0".into(),
            })
        );
    }

    #[test]
    fn a_shared_name_suffixes_every_row_that_has_a_pane() {
        let mut second = row("idle", 10);
        second.zellij = Some(Zellij {
            session: "s".into(),
            pane: "3".into(),
        });
        let snap = snapshot(&[row("idle", 20), second], NOW);
        let titles: Vec<&str> = snap.entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["s:3", "s:0"]);
    }

    #[test]
    fn a_unique_name_takes_no_suffix() {
        let mut second = row("idle", 10);
        second.zellij = Some(Zellij {
            session: "other".into(),
            pane: "3".into(),
        });
        let snap = snapshot(&[row("idle", 20), second], NOW);
        let titles: Vec<&str> = snap.entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["other", "s"]);
    }

    #[test]
    fn only_busy_spins() {
        let busy = snapshot(&[row("busy", 5)], NOW);
        assert!(busy.any_spinning());
        assert_ne!(
            busy.entries[0].status.glyph(0),
            busy.entries[0].status.glyph(1)
        );

        let idle = snapshot(&[row("idle", 5)], NOW);
        assert!(!idle.any_spinning());
        assert_eq!(
            idle.entries[0].status.glyph(0),
            idle.entries[0].status.glyph(7)
        );
    }

    #[test]
    fn a_quiet_badge_is_empty_rather_than_zero() {
        assert_eq!(snapshot(&[row("idle", 5)], NOW).badge().text(), "");
        assert_eq!(snapshot(&[row("waiting", 5)], NOW).badge().text(), "1");
    }

    #[test]
    fn each_badge_state_draws_one_picture() {
        assert_eq!(Badge::Quiet.text(), "");
        assert_eq!(Badge::Waiting(waiting(3)).text(), "3");
        assert_eq!(Badge::Waiting(waiting(3)).rgb(), BLOCKED);
        assert_eq!(Badge::Blind.text(), "\u{2298}");
        assert_eq!(Badge::Blind.rgb(), FAULT);
    }

    #[test]
    fn only_the_quiet_badge_stays_silent() {
        assert!(!Badge::Quiet.needs_attention());
        assert!(Badge::Waiting(waiting(1)).needs_attention());
        assert!(Badge::Blind.needs_attention());
    }

    #[test]
    fn truncation_keeps_the_tail() {
        assert_eq!(truncate("short", 28), "short");
        let long = truncate("projeto-ponte-longissimo-nome-55:12", 28);
        assert_eq!(long.chars().count(), 28);
        assert!(long.ends_with(":12"), "{long}");
        assert!(long.contains('\u{2026}'), "{long}");
    }

    #[test]
    fn ages_read_in_one_unit() {
        assert_eq!(humanise(0), "<1m");
        assert_eq!(humanise(59), "<1m");
        assert_eq!(humanise(60), "1m");
        assert_eq!(humanise(3_599), "59m");
        assert_eq!(humanise(3_600), "1h");
        assert_eq!(humanise(86_400), "1d");
    }
}
