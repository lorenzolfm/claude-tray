//! The mapping from what `claude-ps` says to what the tray shows.
//!
//! This module holds that mapping, and no other module holds a part of it. `claude-ps` passes
//! `status` through unchanged and tells consumers not to compare it against a fixed set, so a
//! consumer must decide. The same code must draw the badge and build the menu, or the count and
//! the list can disagree about one session.
//!
//! The decision comes from `luneta`. The two surfaces read the same producer about the same
//! agents, so a second vocabulary here would make one session two different things. There are
//! four states, and they are the picker's four: `waiting`, `idle`, `busy`, and all other
//! values, in that order. See [`State`].
//!
//! Each function here is pure and takes `now` as an argument, so each rule below is a test.

use crate::agents::Row;

/// Names truncate in the middle. The columns align only if the GTK menu font is monospace,
/// which is a system setting and not a guarantee.
const NAME_WIDTH: usize = 28;

/// One turn of the busy spinner, one frame for each animation tick.
///
/// The frames are braille and not the ASCII `|/-\`, because each frame here is one column wide.
/// The glyph column thus keeps its width as the spinner turns, instead of moving the name column
/// ten times a second for as long as an agent is busy.
///
/// These frames differ from `luneta`'s, and this is the only difference between the two
/// pictures. The picker's cycle is `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, with three dots lit in each frame. That is
/// legible in a terminal but not here, because the GTK theme draws a menu row in its own
/// foreground colour and three dots become grey specks. Colour is not available: Waybar draws
/// this menu through `libdbusmenu-gtk3`, whose `set_label` calls `g_markup_escape_text` on each
/// label, so a `<span foreground>` around the glyph shows as angle brackets. The one colour that
/// dbusmenu gives for a row is `disposition`, and it paints the full label. Weight is thus the
/// only control left: seven dots in each frame instead of three, in the same one-column cell.
///
/// The part that carries meaning stays the same as `luneta`'s: which status turns, and that no
/// other status turns. This cycle has eight frames where the picker's has ten, so one turn takes
/// 0.8 s at `crate::TICK`.
///
/// Each frame ends with a space, and the emoji beside it do not. This is how the column stays
/// aligned: an emoji is two columns wide and a braille cell is one, so the space supplies the
/// second column. The padding is in this table and not in [`Entry::label`], because the format
/// string cannot measure the two widths.
const SPINNER: [&str; 8] = ["⣾ ", "⣽ ", "⣻ ", "⢿ ", "⡿ ", "⣟ ", "⣯ ", "⣷ "];

/// Unknown, and not one of the four on purpose: a status that this build cannot name must not
/// look like one that it can. The glyph comes from `luneta`, like the rest of the table.
const UNKNOWN_GLYPH: &str = "🛸";

/// What the applet decides about one agent: where the row sorts, and whether the badge counts
/// it. The word on the row comes from the producer.
///
/// These are `luneta`'s four ranks. The picker sorts `waiting`, then `idle`, then `busy`, then
/// all other values. The variants below are declared in that order, so `derive(Ord)` gives the
/// sort key and the two surfaces share one order.
///
/// The last variant is the important one: an unknown status becomes `Other`, and the badge never
/// counts `Other`. The status vocabulary is open and changes with Claude Code, which added
/// `shell` between two releases. A new word is thus only a question of time. Last place fails
/// quietly, but a count would make a badge out of a spelling error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// The agent asked a question and stopped. This is the only state that the badge counts.
    Waiting,
    /// The agent finished its turn. The row sorts above the working rows, and the badge does
    /// not count it. See [`State::is_actionable`].
    Idle,
    /// The agent works, and it leaves this state without your help.
    Busy,
    /// `shell`, or a status that this build does not know.
    Other,
}

impl State {
    /// The badge shows the number of actionable agents. Actionable means that the agent waits
    /// for you, and nothing else.
    ///
    /// `idle` is not actionable, and this is the one decision that the applet adds to the
    /// picker's order. A finished turn is sufficient for a row above the working rows, which is
    /// why `Idle` sorts before `Busy`. It is not sufficient for a number in the bar, because
    /// nothing ends that state. An earlier state counted `idle` and needed a timeout of one hour
    /// and a delay for new sessions to keep an old session quiet, and both values were guesses.
    /// A `waiting` agent has a question pending, so the count now has one meaning: an agent
    /// waits for you.
    pub fn is_actionable(self) -> bool {
        matches!(self, State::Waiting)
    }
}

/// Where a row points. `None` when the agent is not in zellij, so there is no destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub session: String,
    pub pane: String,
}

/// One menu row, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub state: State,
    /// The value from the producer, unchanged. [`Entry::glyph`] selects the picture from it,
    /// and the menu prints it beside that picture.
    ///
    /// The word comes from the producer and not from [`State`]. `luneta` prints the status that
    /// it receives. A translation to a local word would show `working` for a status added after
    /// this code, which would claim knowledge of a state that this build has not seen.
    pub raw_status: String,
    /// What this row calls the agent: the name that a person chose, or the zellij session that
    /// holds it if no person chose a name. [`name_rows`] completes this, because a name is only
    /// sufficient in comparison with the other rows in the same menu.
    pub title: String,
    /// Seconds in the current status at the time of the snapshot. The menu shows this value
    /// plus the time since the snapshot. See [`Entry::label`].
    pub age_s: u64,
    pub target: Option<Target>,
}

impl Entry {
    /// The picture on this row.
    ///
    /// The producer's word selects the picture, and `luneta`'s agents tab is the standard. The
    /// [`State`] value does not select it. A choice by state would give the same agent two
    /// different pictures on the two surfaces. The table below is `luneta`'s `agents::glyph`,
    /// including the comparison without case. The [`SPINNER`] frames are the one exception, and
    /// they are heavier and not different.
    ///
    /// `frame` is the animation tick, and it changes one glyph only: the busy spinner. Each
    /// other status gives the same string in each frame, so a repaint at a tick changes only the
    /// rows that turn.
    pub fn glyph(&self, frame: u64) -> &'static str {
        match self.raw_status.to_ascii_lowercase().as_str() {
            // A raised hand. This is the status that the applet exists to show, and it is what
            // the agent does: it asked a question and stopped.
            "waiting" => "🙋",
            // An idle agent has finished and waits for your next instruction, which is why it
            // sorts before a busy one. A cup shows that better than a 💤.
            "idle" => "☕",
            // The one glyph that moves, because it is the one status that changes without your
            // help. The spinner shows that without a look at the age column.
            "busy" => SPINNER[(frame as usize) % SPINNER.len()],
            // A shell.
            "shell" => "🐚",
            _ => UNKNOWN_GLYPH,
        }
    }

    /// Does the glyph on this row move, and is a tick thus worth a repaint?
    ///
    /// The caller asks before it renders, so this is a question of cost and not of correctness.
    /// A wrong answer for an unknown word gives a spinner that does not turn. The comparison is
    /// therefore the same one that [`Entry::glyph`] makes.
    pub fn is_spinning(&self) -> bool {
        self.raw_status.eq_ignore_ascii_case("busy")
    }

    /// One row: `glyph · title · status age`.
    ///
    /// Each row shows an age, and a busy row shows one too. On a busy row the age is the
    /// duration of the turn, and it is the only indication that a session is stuck.
    ///
    /// `since` is the time since the snapshot. It is added here and not stored in
    /// [`Entry::age_s`], because the list is a snapshot and the clock is not. The menu rebuilds
    /// ten times a second while it is open, and its rows are five seconds old at worst. Without
    /// `since`, an agent that has waited three minutes would show the same number for as long as
    /// you look at it. The addition is safe because it is the same number on each row: an equal
    /// offset cannot change a comparison, so the order stays the same and only the ages move.
    pub fn label(&self, frame: u64, since: u64) -> String {
        format!(
            "{}  {:<width$}  {} {}",
            self.glyph(frame),
            truncate(&self.title, NAME_WIDTH),
            self.raw_status,
            humanise(self.age_s.saturating_add(since)),
            width = NAME_WIDTH,
        )
    }
}

/// Everything the tray needs for one repaint, derived once so the badge and the list cannot
/// disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// In `luneta`'s order: attention first, and most recent first within one status.
    pub entries: Vec<Entry>,
    /// The number of actionable agents, which are the agents that wait for you.
    pub badge: usize,
    /// Unix seconds at the time of the snapshot, so that [`Entry::label`] can show its age.
    pub taken_at: u64,
}

impl Snapshot {
    /// What the pixmap shows beside the mark. It is empty when there is no work, because the
    /// quiet state is the mark alone. A `0` is still something to read.
    ///
    /// Each agent in this number waits for you, so there is one badge colour. See
    /// `crate::icon::BLOCKED`.
    pub fn badge_text(&self) -> String {
        if self.badge == 0 {
            String::new()
        } else {
            self.badge.to_string()
        }
    }

    /// Is there a spinner in this menu?
    ///
    /// The test covers each row, because the menu draws each row. There is no scroll window that
    /// could make this answer wrong.
    pub fn any_spinning(&self) -> bool {
        self.entries.iter().any(Entry::is_spinning)
    }

    /// The age of this snapshot, in seconds. A clock that moves backwards gives an age of zero,
    /// so the ages stop instead of jumping.
    pub fn since(&self, now: u64) -> u64 {
        now.saturating_sub(self.taken_at)
    }
}

/// Classify one row, which decides only where the row sorts. The word beside the glyph comes
/// from the producer.
///
/// The comparison ignores case, because `luneta` compares each status with
/// `eq_ignore_ascii_case` and its table is the standard. An exact comparison here would let
/// `Idle` sort second on one surface and last on the other.
fn classify(status: &str) -> State {
    match status.to_ascii_lowercase().as_str() {
        "waiting" => State::Waiting,
        "idle" => State::Idle,
        "busy" => State::Busy,
        _ => State::Other,
    }
}

/// Turn a poll into a repaint.
///
/// The order is this module's responsibility. `claude-ps` sorts by pid, so that two runs one
/// second apart give a clean difference, and its README says that this order is for comparison
/// and not for reading.
///
/// The order here comes from `luneta`, in both parts. Attention comes first, by [`State`]'s own
/// `Ord`. Within one status, the most recent agent comes first, because the agent that changed a
/// moment ago is the agent that you last worked with.
///
/// Equal ages keep the producer's pid order, because `sort_by` is stable. Two agents that
/// changed in the same second thus do not exchange places between polls.
pub fn snapshot(rows: &[Row], now: u64) -> Snapshot {
    let mut entries: Vec<Entry> = rows
        .iter()
        .map(|row| Entry {
            state: classify(&row.raw_status),
            raw_status: row.raw_status.clone(),
            // Decided here, in three steps, because it depends on this row alone. Only the
            // `:pane` suffix needs the other rows, and `name_rows` adds it.
            title: chosen_name(&row.name, row.name_source.as_deref())
                .or_else(|| row.zellij.as_ref().map(|z| z.session.clone()))
                .unwrap_or_else(|| row.name.clone()),
            age_s: row.transition_age_s,
            // One `map`, because the producer sends the pair as one object. There is no state
            // where a session is known and its pane is not.
            target: row.zellij.as_ref().map(|z| Target {
                session: z.session.clone(),
                pane: z.pane.clone(),
            }),
        })
        .collect();

    entries.sort_by(|a, b| a.state.cmp(&b.state).then(a.age_s.cmp(&b.age_s)));
    name_rows(&mut entries);

    let badge = entries.iter().filter(|e| e.state.is_actionable()).count();

    Snapshot {
        entries,
        badge,
        taken_at: now,
    }
}

/// The name that a person chose for this agent, or `None` to use another value.
///
/// `claude-ps` reports the name and who chose it, and the second value is the important one. A
/// `derived` name is the basename of the cwd plus a suffix, so a row with that name would carry
/// the name of a directory that holds it by chance. Only `user` and `peer` mean that a person or
/// another agent chose the name.
///
/// An unknown source is suppressed, which is the opposite of what [`classify`] does with an
/// unknown status. The producer causes that difference, and it is correct on both sides. Each
/// value in the status vocabulary is a real state, so a hidden value hides a live agent. But the
/// sources that carry a chosen name are a short closed list, and the sources that carry a
/// generated name are a long open one: Claude Code already writes `derived`, `collision`, `auto`
/// and `hook`. A new source is more probably a generated name, and trust in it would put a
/// generated name where a chosen name belongs.
///
/// `None` is trusted, because it is the state from before the key existed and not a source that
/// this build failed to recognise. An older `claude-ps` must continue to work.
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

/// Add the pane to each row whose name no longer identifies one row in the menu.
///
/// `snapshot` decides the name in the picker's three steps: the chosen name; then the zellij
/// session; then Claude Code's label, for an agent that has neither. This function adds the
/// suffix.
///
/// Two rules apply, and both come from `luneta`:
///
/// - Compare the names over the rows that the menu shows. The menu hides no row, so that is each
///   entry. The suffix thus appears when two rows would otherwise look the same.
/// - When two rows share a name, each of those rows takes a suffix. A rule that leaves the first
///   row without one is not a rule that a person can see in the menu.
///
/// A suffix needs a pane, so only a row in zellij can take one. An agent outside zellij keeps
/// its plain name, because `-:-` would be worse than the collision that it reports.
///
/// Each comparison occurs before each rename. A rename in the same pass would compare the plain
/// name of the second row against the suffixed name of the first row, find no collision, and
/// keep the plain name that the suffix must separate.
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

/// Ages in one unit: `<1m`, `47m`, `3h`, `2d`. More precision has no value, because the
/// question is only whether the agent has waited for a long time.
fn humanise(secs: u64) -> String {
    match secs {
        0..60 => "<1m".to_string(),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// Truncate in the middle, because the end of the name must stay. A `:pane` suffix is the only
/// difference between two rows in one session, and the names for agents outside zellij are cwd
/// basenames: `projeto-ponte-55` and `projeto-ponte-61` differ only at the end.
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
    use crate::agents::Zellij;

    const NOW: u64 = 1_800_000_000;

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

    /// `luneta`'s rank, and the reason for the order of [`State`]'s variants.
    #[test]
    fn the_states_rank_in_the_pickers_order() {
        assert_eq!(classify("waiting"), State::Waiting);
        assert_eq!(classify("idle"), State::Idle);
        assert_eq!(classify("busy"), State::Busy);
        assert_eq!(classify("shell"), State::Other);

        assert!(State::Waiting < State::Idle);
        assert!(State::Idle < State::Busy);
        assert!(State::Busy < State::Other);
    }

    /// The picker compares with `eq_ignore_ascii_case`. This module must do the same, or one
    /// word ranks differently on the two surfaces.
    #[test]
    fn a_status_is_read_case_insensitively() {
        assert_eq!(classify("WAITING"), State::Waiting);
        assert_eq!(classify("Idle"), State::Idle);
    }

    /// The rule for a difference of versions. A status that this build does not know must go
    /// to a safe place, and last and uncounted is the only safe place.
    #[test]
    fn an_unknown_status_ranks_last_and_is_never_counted() {
        for unknown in ["shell", "compacting", "", "nonsense"] {
            let s = classify(unknown);
            assert_eq!(s, State::Other, "{unknown:?}");
            assert!(!s.is_actionable(), "{unknown:?}");
        }
    }

    /// A finished turn sorts above a working turn, and the badge does not count it at any age.
    /// An earlier design counted it and needed two thresholds to keep a new session and an old
    /// session quiet. Neither threshold is necessary now.
    #[test]
    fn an_idle_agent_is_never_counted_at_any_age() {
        for age in [0, 5, 3_600, 7 * 86_400] {
            let snap = snapshot(&[row("idle", age)], NOW);
            assert_eq!(snap.badge, 0, "{age}s");
            assert_eq!(snap.entries[0].state, State::Idle, "{age}s");
        }
    }

    /// The rule that the applet keeps: the number in the bar is the number of agents with a
    /// question pending.
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
        assert_eq!(snap.badge, 2);
        assert_eq!(snap.entries.len(), 5, "nothing is hidden, only uncounted");
    }

    /// `luneta`'s order, in both parts: attention first, then most recent first within one
    /// status.
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
            .map(|e| (e.state, e.age_s))
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

    /// Equal ages keep the producer's pid order, so two agents that changed in the same second
    /// do not exchange places between two polls.
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

    /// The clock moves when the list does not. The menu rebuilds from one snapshot ten times a
    /// second while it is open. Without the offset, each row would show the age from the last
    /// run of `claude-ps`.
    #[test]
    fn the_age_counts_on_after_the_snapshot_was_taken() {
        let snap = snapshot(&[row("waiting", 59)], NOW);
        assert_eq!(snap.since(NOW), 0);
        assert!(snap.entries[0].label(0, 0).ends_with("<1m"));
        assert_eq!(snap.since(NOW + 90), 90);
        assert!(snap.entries[0].label(0, 90).ends_with("2m"));
    }

    /// A clock that moves backwards stops the ages instead of moving them.
    #[test]
    fn a_backwards_clock_reads_as_fresh() {
        assert_eq!(snapshot(&[row("idle", 10)], NOW).since(NOW - 500), 0);
    }

    /// The word beside the glyph comes from the producer, so a status added after this code
    /// shows its own name and not a local translation.
    #[test]
    fn the_row_prints_the_status_it_was_given() {
        let snap = snapshot(&[row("compacting", 120)], NOW);
        let label = snap.entries[0].label(0, 0);
        assert!(label.contains("compacting"), "{label}");
        assert!(label.starts_with(UNKNOWN_GLYPH), "{label}");
    }

    /// A `derived` name is the cwd basename plus a suffix, so the row takes the name of its
    /// address.
    #[test]
    fn a_derived_name_loses_to_the_zellij_session() {
        assert_eq!(snapshot(&[row("idle", 5)], NOW).entries[0].title, "s");
    }

    /// A name that a person chose wins, because it is the only string in the row that gives the
    /// purpose of the agent.
    #[test]
    fn a_chosen_name_wins_over_the_session() {
        for source in ["user", "peer", "USER"] {
            let mut r = row("idle", 5);
            r.name_source = Some(source.into());
            assert_eq!(snapshot(&[r], NOW).entries[0].title, "n", "{source}");
        }
    }

    /// An absent source is the state from before Claude Code recorded one, so it is trusted. A
    /// source that this build does not know is suppressed, because the generated names are the
    /// long open list.
    #[test]
    fn an_absent_source_is_trusted_and_an_unknown_one_is_not() {
        let mut absent = row("idle", 5);
        absent.name_source = None;
        assert_eq!(snapshot(&[absent], NOW).entries[0].title, "n");

        let mut unknown = row("idle", 5);
        unknown.name_source = Some("something-new".into());
        assert_eq!(snapshot(&[unknown], NOW).entries[0].title, "s");
    }

    /// An agent outside zellij has no session to give it a name, so Claude Code's label is its
    /// only identifier.
    #[test]
    fn an_agent_outside_zellij_keeps_the_label_it_has() {
        let mut r = row("idle", 5);
        r.zellij = None;
        let snap = snapshot(&[r], NOW);
        assert_eq!(snap.entries[0].title, "n");
        assert_eq!(snap.entries[0].target, None);
    }

    /// When two rows share a name, each of those rows takes a suffix. A rule that leaves the
    /// first row without one is not a rule that a person can see in the menu.
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

    /// A name that identifies one row does not change.
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

    /// Only the glyph on a busy row moves, and it is the only reason for a repaint at a tick.
    #[test]
    fn only_busy_spins() {
        let busy = snapshot(&[row("busy", 5)], NOW);
        assert!(busy.any_spinning());
        assert_ne!(busy.entries[0].glyph(0), busy.entries[0].glyph(1));

        let idle = snapshot(&[row("idle", 5)], NOW);
        assert!(!idle.any_spinning());
        assert_eq!(idle.entries[0].glyph(0), idle.entries[0].glyph(7));
    }

    /// The quiet state is the mark alone and not a `0`, because a zero is still something to
    /// read.
    #[test]
    fn a_quiet_badge_is_empty_rather_than_zero() {
        assert_eq!(snapshot(&[row("idle", 5)], NOW).badge_text(), "");
        assert_eq!(snapshot(&[row("waiting", 5)], NOW).badge_text(), "1");
    }

    /// The end of the name must stay: it holds a `:pane` suffix, and it is where two names from
    /// a cwd differ.
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
