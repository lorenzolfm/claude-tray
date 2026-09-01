//! The mapping from what `claude-ps` says to what the tray shows.
//!
//! This module holds that mapping, and no other module holds a part of it. `claude-ps` passes
//! `status` through unchanged and tells consumers not to compare it against a fixed set, so a
//! consumer must decide. The same code must draw the badge and build the menu, or the count and
//! the list can disagree about one session.
//!
//! One word gives three answers: where the row sorts, which picture it draws, and which word it
//! prints. [`Status`] holds the three together and reads the word once, so that a row cannot
//! rank as one status and read as another.
//!
//! The decision comes from `luneta`. The two surfaces read the same producer about the same
//! agents, so a second vocabulary here would make one session two different things. There are
//! four states, and they are the picker's four: `waiting`, `idle`, `busy`, and all other
//! values, in that order. See [`State`].
//!
//! Each function here is pure and takes `now` as an argument, so each rule below is a test.

use crate::agents::{Row, Zellij};
use crate::icon::{BLOCKED, FAULT};
use std::num::NonZeroUsize;

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

/// A status word from the producer, and everything that this build concludes from it.
///
/// The word is read once, in [`Status::parse`], and the three answers that follow from it stay
/// together: the rank, the picture and the word itself. Three separate fields could disagree,
/// and a row that sorts first, counts in the badge and prints `idle` beside a ☕ is the failure
/// that this module exists to prevent.
///
/// The fields are private for that reason. [`Status::parse`] is the only way to a value, so the
/// rank and the picture always come from the word that the row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    raw: String,
    state: State,
    face: Face,
}

/// The picture for a status, decided beside the rank.
///
/// `Spinner` is the one face that moves, and it is the only reason that an animation tick is
/// worth a repaint. It is a variant and not a string in the table, because the frame is not
/// known when the word is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    Fixed(&'static str),
    Spinner,
}

impl Status {
    /// Read one status word, and decide the rank and the picture from it together.
    ///
    /// The comparison ignores case, because `luneta` compares each status with
    /// `eq_ignore_ascii_case` and its table is the standard. An exact comparison here would let
    /// `Idle` sort second on one surface and last on the other.
    ///
    /// The picture vocabulary is finer than the rank vocabulary, which is why the two answers
    /// come from one table instead of one from the other. `shell` has a picture of its own and
    /// still ranks last, with the words that this build does not know.
    ///
    /// The last arm is the important one: an unknown status becomes `Other` with the unknown
    /// glyph, and the badge never counts `Other`. The status vocabulary is open and changes with
    /// Claude Code, which added `shell` between two releases, so a new word is only a question
    /// of time. Last place fails quietly, but a count would make a badge out of a spelling
    /// error, and a known picture would make a new status look like an old one.
    pub fn parse(raw: String) -> Self {
        let (state, face) = match raw.to_ascii_lowercase().as_str() {
            // A raised hand. This is the status that the applet exists to show, and it is what
            // the agent does: it asked a question and stopped.
            "waiting" => (State::Waiting, Face::Fixed("🙋")),
            // An idle agent has finished and waits for your next instruction, which is why it
            // sorts before a busy one. A cup shows that better than a 💤.
            "idle" => (State::Idle, Face::Fixed("☕")),
            // The one face that moves, because it is the one status that changes without your
            // help. The spinner shows that without a look at the age column.
            "busy" => (State::Busy, Face::Spinner),
            // A shell. It has its own picture and no rank of its own.
            "shell" => (State::Other, Face::Fixed("🐚")),
            _ => (State::Other, Face::Fixed(UNKNOWN_GLYPH)),
        };
        Self { raw, state, face }
    }

    /// Where a row with this status sorts, and whether the badge counts it.
    pub fn state(&self) -> State {
        self.state
    }

    /// The word from the producer, unchanged, for the menu to print beside the picture.
    ///
    /// The word comes from the producer and not from [`State`]. `luneta` prints the status that
    /// it receives. A translation to a local word would show `working` for a status added after
    /// this code, which would claim knowledge of a state that this build has not seen.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The picture for this status.
    ///
    /// The producer's word selected it, in [`Status::parse`], and `luneta`'s agents tab is the
    /// standard. The [`State`] value does not select it. A choice by state would give the same
    /// agent two different pictures on the two surfaces, and it would lose the picture for
    /// `shell`, which ranks with the words that this build does not know.
    ///
    /// `frame` is the animation tick, and it changes one picture only: the busy spinner. Each
    /// other status gives the same string in each frame, so a repaint at a tick changes only the
    /// rows that turn.
    pub fn glyph(&self, frame: u64) -> &'static str {
        match self.face {
            Face::Fixed(glyph) => glyph,
            Face::Spinner => SPINNER[(frame as usize) % SPINNER.len()],
        }
    }

    /// Does the picture on this row move, and is a tick thus worth a repaint?
    ///
    /// The caller asks before it renders, so this is a question of cost and not of correctness.
    /// The answer cannot differ from [`Status::glyph`], because both read the same [`Face`]: a
    /// status that reports a spinner draws one, and a status that draws one reports it.
    pub fn is_spinning(&self) -> bool {
        matches!(self.face, Face::Spinner)
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
    /// What the producer said about this agent, and what this build concluded from it: the
    /// rank, the picture and the word. They were decided together, so they cannot disagree.
    pub status: Status,
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
            self.status.glyph(frame),
            truncate(&self.title, NAME_WIDTH),
            self.status.as_str(),
            humanise(self.age_s.saturating_add(since)),
            width = NAME_WIDTH,
        )
    }
}

/// What goes beside the mark, and in which colour. The mark is not part of this. It is
/// identity, and it stays [`crate::mark::CLAUDE`] in each state. Three pictures can appear
/// here, and each one has one meaning:
///
/// - [`Badge::Quiet`] draws nothing, which shows the mark alone. Nothing waits for you.
/// - [`Badge::Waiting`] draws the count in [`BLOCKED`] amber. Each agent in that number waits
///   for you.
/// - [`Badge::Blind`] draws `⊘` in [`FAULT`] red. It does not mean "you have work". It
///   means that the applet cannot see, which a producer that is absent or that exits non-zero
///   causes.
///
/// The text and the colour are one value and not a pair, because only three of the pairs that a
/// `(String, [u8; 3])` admits have a meaning. An amber `⊘` is a blind applet that reads as
/// work, and a red count is a number in the colour that says "this is not a number". Both were
/// forbidden by comment until this type made them unconstructible.
///
/// [`Badge::Waiting`] carries a [`NonZeroUsize`] for the same reason. A count of `0` and an
/// empty text are one state, and it used to be spelled twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    /// Nothing waits for you, so the bar shows the mark alone.
    Quiet,
    /// This many agents wait for you. The number is never `0`, because that state is
    /// [`Badge::Quiet`].
    Waiting(NonZeroUsize),
    /// The applet cannot see. [`crate::agents::poll`] failed, so there are no rows to count and
    /// a quiet mark would be a lie.
    Blind,
}

impl Badge {
    /// What `crate::icon::Renderer::render` draws beside the mark.
    pub fn text(self) -> String {
        match self {
            Badge::Quiet => String::new(),
            Badge::Waiting(n) => n.to_string(),
            Badge::Blind => "\u{2298}".to_string(),
        }
    }

    /// The colour of that text. The colour is in the pixels because CSS cannot supply it; see
    /// [`crate::icon`].
    ///
    /// [`Badge::Quiet`] has no text, so its colour never reaches a pixel. It answers with the
    /// amber anyway. An `Option` here would make each caller unwrap a colour for a picture that
    /// does not exist.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Badge::Quiet | Badge::Waiting(_) => BLOCKED,
            Badge::Blind => FAULT,
        }
    }

    /// Does this state ask you to look? [`Badge::Quiet`] is the only state that does not.
    ///
    /// A failed producer asks. It keeps the rule that amber means work, because that state has
    /// no count: the applet draws `⊘` instead of a number, so nobody can read the colour as
    /// a count. Without this, the applet would look quiet while it cannot see.
    pub fn needs_attention(self) -> bool {
        !matches!(self, Badge::Quiet)
    }
}

/// Everything the tray needs for one repaint: the rows, and the moment that they describe.
///
/// The badge is not one of the fields. [`Snapshot::badge`] counts the rows at each read, so the
/// number in the bar and the list in the menu cannot disagree about one session. That is the
/// failure that this module's header names, and a cached count is how it happens: a `Snapshot`
/// built with the wrong one would compile.
///
/// The count does not need a cache. The menu rebuilds ten times a second over a handful of
/// rows, so one `filter().count()` for each read is free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// In `luneta`'s order: attention first, and most recent first within one status.
    pub entries: Vec<Entry>,
    /// Unix seconds at the time of the snapshot, so that [`Entry::label`] can show its age.
    pub taken_at: u64,
}

impl Snapshot {
    /// What goes beside the mark, derived from the rows that this snapshot holds.
    ///
    /// A snapshot exists only where the producer answered, so this never gives
    /// [`Badge::Blind`]. That state belongs to the tray, which is the one place that knows
    /// about a failed poll.
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

    /// Is there a spinner in this menu?
    ///
    /// The test covers each row, because the menu draws each row. There is no scroll window that
    /// could make this answer wrong.
    pub fn any_spinning(&self) -> bool {
        self.entries.iter().any(|entry| entry.status.is_spinning())
    }

    /// The age of this snapshot, in seconds. A clock that moves backwards gives an age of zero,
    /// so the ages stop instead of jumping.
    pub fn since(&self, now: u64) -> u64 {
        now.saturating_sub(self.taken_at)
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
            status: Status::parse(row.raw_status.clone()),
            // Decided here, in three steps, because it depends on this row alone. Only the
            // `:pane` suffix needs the other rows, and `name_rows` adds it.
            title: chosen_name(&row.name, row.name_source.as_deref())
                .or_else(|| row.zellij.as_ref().map(|z| z.session.clone()))
                .unwrap_or_else(|| row.name.clone()),
            age_s: row.transition_age_s,
            // One `and_then`, because the producer sends the pair as one object and
            // `Zellij::address` judges it as one. There is no state where a session is known
            // and its pane is not, and none where half of an address is usable. The title
            // above still reads the raw session, so a row that no click can reach keeps the
            // name that says which agent it is.
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

/// The name that a person chose for this agent, or `None` to use another value.
///
/// `claude-ps` reports the name and who chose it, and the second value is the important one. A
/// `derived` name is the basename of the cwd plus a suffix, so a row with that name would carry
/// the name of a directory that holds it by chance. Only `user` and `peer` mean that a person or
/// another agent chose the name.
///
/// An unknown source is suppressed, which is the opposite of what [`Status::parse`] does with
/// an unknown status. The producer causes that difference, and it is correct on both sides. Each
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

    const NOW: u64 = 1_800_000_000;

    /// Where one status word ranks. The rank is now one of three answers that
    /// [`Status::parse`] gives, and the tests below ask for it alone.
    fn rank(status: &str) -> State {
        Status::parse(status.into()).state()
    }

    /// A count for [`Badge::Waiting`]. Each one in the tests below is a literal that is not
    /// zero, so the panic is unreachable.
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

    /// `luneta`'s rank, and the reason for the order of [`State`]'s variants.
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

    /// The picker compares with `eq_ignore_ascii_case`. This module must do the same, or one
    /// word ranks differently on the two surfaces.
    #[test]
    fn a_status_is_read_case_insensitively() {
        assert_eq!(rank("WAITING"), State::Waiting);
        assert_eq!(rank("Idle"), State::Idle);
    }

    /// The rule for a difference of versions. A status that this build does not know must go
    /// to a safe place, and last and uncounted is the only safe place.
    #[test]
    fn an_unknown_status_ranks_last_and_is_never_counted() {
        for unknown in ["shell", "compacting", "", "nonsense"] {
            let s = rank(unknown);
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
            assert_eq!(snap.badge(), Badge::Quiet, "{age}s");
            assert_eq!(snap.entries[0].status.state(), State::Idle, "{age}s");
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
        assert_eq!(snap.badge(), Badge::Waiting(waiting(2)));
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

    /// The picture vocabulary is finer than the rank vocabulary, so a rank cannot select a
    /// picture. `shell` has a picture of its own on both surfaces and still ranks last, and a
    /// build that derived the one from the other would draw it as a word it does not know.
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

    /// The address `claude-ps` sends is not always one that a click can use, and a row that
    /// cannot be reached must say so by going grey. A session name that holds a space is the
    /// writer: `pair_sockets` splits the `ss` rows on spaces, never joins that client to its
    /// server, and `focus` then attaches — a second terminal for a session already on screen.
    /// The name survives, because it is still the only word that says which agent the row is.
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

        // And the title fallback reaches the raw session when no person named the agent.
        let mut derived = row("idle", 5);
        derived.zellij = Some(Zellij {
            session: "my work".into(),
            pane: "0".into(),
        });
        let snap = snapshot(&[derived], NOW);
        assert_eq!(snap.entries[0].target, None);
        assert_eq!(snap.entries[0].title, "my work");
    }

    /// An empty string is not an address either, and half an address is none: the pair is
    /// judged as the one object the producer sends it as.
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

    /// The other half of that rule: an address that the jump can act on is passed through
    /// whole, so the guard above rejects and does not merely narrow.
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

    /// The quiet state is the mark alone and not a `0`, because a zero is still something to
    /// read. The type says it now: there is no `Waiting(0)` to draw.
    #[test]
    fn a_quiet_badge_is_empty_rather_than_zero() {
        assert_eq!(snapshot(&[row("idle", 5)], NOW).badge().text(), "");
        assert_eq!(snapshot(&[row("waiting", 5)], NOW).badge().text(), "1");
    }

    /// The three pictures that can appear beside the mark, and the colour of each one. The
    /// amber `⊘` and the red count are not tested, because neither can be built.
    #[test]
    fn each_badge_state_draws_one_picture() {
        assert_eq!(Badge::Quiet.text(), "");
        assert_eq!(Badge::Waiting(waiting(3)).text(), "3");
        assert_eq!(Badge::Waiting(waiting(3)).rgb(), BLOCKED);
        assert_eq!(Badge::Blind.text(), "\u{2298}");
        assert_eq!(Badge::Blind.rgb(), FAULT);
    }

    /// A failed producer asks you to look, and the quiet mark does not. `ClaudeTray::status`
    /// reads this and nothing else, so the three states of the badge and the two SNI statuses
    /// are decided in one place.
    #[test]
    fn only_the_quiet_badge_stays_silent() {
        assert!(!Badge::Quiet.needs_attention());
        assert!(Badge::Waiting(waiting(1)).needs_attention());
        assert!(Badge::Blind.needs_attention());
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
