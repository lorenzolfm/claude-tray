//! The mapping from what `claude-ps` says to what the tray shows.
//!
//! 🔴 **This is the one place the mapping lives.** `claude-ps` passes `status` through
//! verbatim and tells consumers not to match it against a fixed set, so somebody downstream has
//! to decide — and it has to be the same somebody that draws the badge and builds the menu, or
//! the count and the list can disagree about the same session.
//!
//! 🔴 **And the decision is `luneta`'s, taken whole.** The two surfaces read the same producer
//! about the same agents, so an applet that invented a second vocabulary made the same session
//! two different things depending on where it was read. What used to live here — a `your turn`
//! that aged out at an hour, a `dormant` that a newborn session was quietly filed under — was
//! judgment this end had no business holding on its own. There are four states now and they are
//! the picker's four: `waiting`, `idle`, `busy`, and everything else, in that order. See
//! [`State`].
//!
//! Everything here is pure and takes `now` as an argument, so every rule below is a test rather
//! than a thing you have to run the applet to see.

use crate::agents::Row;

/// Names middle-truncate here. ⚠️ The columns line up only because the GTK menu font on this
/// box happens to be monospace — a system setting, not a guarantee.
const NAME_WIDTH: usize = 28;

/// One turn of the busy spinner, a frame per animation tick.
///
/// Braille rather than the ASCII `|/-\`: every frame here is **one column wide**, so the glyph
/// column keeps its width as the spinner turns rather than shoving the name column back and
/// forth ten times a second for the whole time an agent is busy.
///
/// 🔴 **These are not `luneta`'s frames — the one place this end's picture deliberately
/// departs from it.** Over there the cycle is `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, three dots lit per frame, which is
/// legible in a terminal and is not legible here: a menu row is drawn in the GTK theme's own
/// foreground, and three dots of it read as grey lint that may or may not be moving. Colour
/// would have been the smaller change and **is not available** — Waybar draws this menu through
/// `libdbusmenu-gtk3`, whose `set_label` runs `g_markup_escape_text` over every label, so a
/// `<span foreground>` around the glyph arrives as literal angle brackets. The one per-row
/// colour dbusmenu does offer is `disposition`, and it paints the *whole* label: a red name and
/// age to fix a grey spinner. So weight is the lever that was left — seven dots lit per frame
/// instead of three, in the same one-column cell.
///
/// ⚠️ What stays shared with `luneta` is the half that carries meaning: **which status spins,
/// and that nothing else does**. Somebody who reads the picker reads this menu right; the frames
/// are heavier, not different. The cycle is eight frames where the picker's is ten, so a turn
/// takes 0.8 s at `crate::TICK` rather than a round second — a spinner has no speed anyone reads.
///
/// ⚠️ Each frame carries a **trailing space** and the emoji beside it do not. That is the whole
/// of how this column stays aligned: an emoji is two columns wide where a braille cell is one,
/// so the space is the second column the spinner would otherwise be missing. `luneta`
/// measures its tag column with `unicode-width`; a dependency to measure five known strings is
/// not the trade here, and it is why the padding lives *in the table* rather than in
/// [`Entry::label`] — the format string below cannot tell the two widths apart.
const SPINNER: [&str; 8] = ["⣾ ", "⣽ ", "⣻ ", "⢿ ", "⡿ ", "⣟ ", "⣯ ", "⣷ "];

/// Unidentified, and deliberately not one of the four — a status this build cannot name must
/// not be able to pass itself off as one it can. `luneta`'s, like the rest of the table.
const UNKNOWN_GLYPH: &str = "🛸";

/// What the applet believes about one agent — which is now only *where it sorts and whether it
/// is counted*, because the word on the row is the producer's own.
///
/// 🔴 **These are `luneta`'s four ranks and nothing more.** The picker orders `waiting`, then
/// `idle`, then `busy`, then anything else, and this enum's `Ord` **is** that order — the
/// variants are declared in it, so `derive(Ord)` is the sort key. Two surfaces, one order.
///
/// 🔴 The last variant is the load-bearing one: **anything unrecognised lands in `Other`, and
/// `Other` is never counted.** The status vocabulary is open and moves with Claude Code — the
/// set grew by one (`shell`) between two releases already — so a word nobody has seen yet is a
/// matter of time. Ranking it last fails silent; letting it count would invent a badge out of a
/// typo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// Its hand is up: it asked something and stopped. 🔴 The only counted state.
    Waiting,
    /// It finished its turn. Listed above what is still running, and never counted — see
    /// [`State::is_actionable`].
    Idle,
    /// It is working, and will leave this state without you.
    Busy,
    /// `shell`, or a status this build has never heard of.
    Other,
}

impl State {
    /// The badge is `count(actionable)`, and this is what actionable means: **blocked on him**,
    /// and nothing else.
    ///
    /// 🔴 `idle` is deliberately *not* here, and this is the one judgment the applet still makes
    /// on top of the picker's order. A finished turn is worth a row above the working ones — it
    /// is why `Idle` outranks `Busy` — but it is not worth a number in the bar, because nothing
    /// retires it: the `your turn` state that used to count it needed an hour-long timeout and a
    /// newborn suppression to stop a session he finished with on Tuesday from nagging on
    /// Thursday, and both of those were guesses. A `waiting` agent has an actual question
    /// pending, so the count means one thing again: **somebody is blocked on you**.
    pub fn is_actionable(self) -> bool {
        matches!(self, State::Waiting)
    }
}

/// Where a row points. `None` when the agent is not inside zellij, so there is nowhere to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub session: String,
    pub pane: String,
}

/// One menu row, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub state: State,
    /// Verbatim from the producer: what [`Entry::glyph`] draws the row's picture from, **and**
    /// the word printed beside it.
    ///
    /// 🔴 The word is the producer's now, where it used to be [`State`]'s. `luneta` prints the
    /// status it was given and lets the glyph be the decoration; an applet that printed
    /// `needs input` beside 🙋 was translating a vocabulary that does not need translating, and
    /// a status invented after this was written would have arrived as `working` — a word
    /// claiming to know something about a state nobody here has seen.
    pub raw_status: String,
    /// What this row calls the agent: the name a **person** chose, or the **zellij session** it
    /// is sitting in when nobody chose one. Filled in by [`name_rows`], because a name can only
    /// be judged sufficient against the other rows in the same menu.
    pub title: String,
    /// Seconds in the current status **when the snapshot was taken**. What renders is this plus
    /// how long ago that was — see [`Entry::label`].
    pub age_s: u64,
    pub target: Option<Target>,
}

impl Entry {
    /// The picture on this row.
    ///
    /// 🔴 **Keyed by the producer's word, and `luneta`'s agents tab is the standard.** Not by
    /// [`State`] — which is tempting, because the states are what this applet sorts by, but it
    /// would mean the same agent wore two different faces depending on which surface it was read
    /// on. One vocabulary, one meaning per picture, and this end of it does not get a vote. The
    /// table below is `luneta`'s `agents::glyph`, case-insensitivity included — [`SPINNER`]'s
    /// frames are the single exception, and they are heavier rather than other.
    ///
    /// `frame` is the animation tick, and it reaches exactly one glyph: the busy spinner. Every
    /// other status returns the same string on every frame, which is what lets the tray repaint
    /// on a tick and have nothing but the turning rows change.
    pub fn glyph(&self, frame: u64) -> &'static str {
        match self.raw_status.to_ascii_lowercase().as_str() {
            // Someone with their hand up: the one status the whole applet exists to surface,
            // and literally what the agent is doing — it has asked something and stopped.
            "waiting" => "🙋",
            // Not asleep. An idle agent has *finished* and is waiting on your next instruction,
            // which is why it outranks a busy one — so it gets a cup rather than the "do not
            // disturb" of a 💤.
            "idle" => "☕",
            // The one glyph that moves, because it is the one status that is *going* somewhere:
            // a busy agent will leave it without you, and the spinner is the row saying so
            // without you having to read the age to find out.
            "busy" => SPINNER[(frame as usize) % SPINNER.len()],
            // It is a shell. There was never going to be another choice.
            "shell" => "🐚",
            _ => UNKNOWN_GLYPH,
        }
    }

    /// Is this the row whose glyph moves — and therefore is a tick worth a repaint?
    ///
    /// Asked before rendering, so it is a question about cost rather than about correctness:
    /// getting it wrong on a word this build has never seen costs a still spinner nobody is
    /// looking at. Which is why it is the same comparison [`Entry::glyph`] makes, and not a
    /// bigger idea.
    pub fn is_spinning(&self) -> bool {
        self.raw_status.eq_ignore_ascii_case("busy")
    }

    /// [[CSB-2]]'s row: `glyph · title · status age`.
    ///
    /// `host` is gone — one machine in this slice. `cwd` never renders. **Every** row carries
    /// an age, busy rows included: on a busy row the age is turn duration, and it is the only
    /// place a wedged session becomes visible at all.
    ///
    /// 🔴 `since` is how long ago the snapshot was taken, and it is added here rather than baked
    /// into [`Entry::age_s`] for the reason `luneta` learnt it: **the list is a glance and the
    /// clock is not.** The menu is rebuilt ten times a second while it is open and the rows in
    /// it are five seconds old at worst, so without this an agent that has been waiting three
    /// minutes reads the same number for as long as you look at it — on the one column that says
    /// whether anything is stuck. Adding it is safe *because* it is the same number on every
    /// row: a uniform offset cannot flip a comparison, so the ordering below lands exactly where
    /// it did and only the ages move.
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
    /// In `luneta`'s order: attention first, most recent first within a status.
    pub entries: Vec<Entry>,
    /// `count(actionable)` — the agents that are *blocked on him*.
    pub badge: usize,
    /// Unix seconds this snapshot was taken at, so [`Entry::label`] can say how stale it is.
    pub taken_at: u64,
}

impl Snapshot {
    /// What the pixmap spells beside the mark. **Empty when there is nothing to do** — calm is
    /// the bare mark, not a `0`, because a zero is still something to read.
    ///
    /// 🔴 This used to prefix a glyph (`◇`/`◆ 3`/`◈ 2`). The mark took that slot, so the
    /// blocked-vs-merely-finished distinction moved into the *colour* of this count — and then
    /// out of it again, when `idle` stopped being counted: everything in this number is blocked
    /// now, so there is one badge colour left. See `crate::icon::BLOCKED`.
    pub fn badge_text(&self) -> String {
        if self.badge == 0 {
            String::new()
        } else {
            self.badge.to_string()
        }
    }

    /// Is there a spinner in this menu to turn?
    ///
    /// Over every row, because the menu draws every row — there is no scrolled-to window here
    /// for this to be wrong about.
    pub fn any_spinning(&self) -> bool {
        self.entries.iter().any(Entry::is_spinning)
    }

    /// How stale this snapshot is, in seconds. A clock that has gone backwards reads as fresh,
    /// which is the quiet direction: the ages stop advancing rather than jumping.
    pub fn since(&self, now: u64) -> u64 {
        now.saturating_sub(self.taken_at)
    }
}

/// Classify one row — which is now only *where it sorts*, since the word beside the glyph is the
/// producer's own.
///
/// 🔴 Lowercased because `luneta` lowercases: it compares every status with
/// `eq_ignore_ascii_case`, and its table is the standard for what a status *is*. Matching
/// exactly here instead would let a hypothetical `Idle` rank second on one surface and last on
/// the other.
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
/// Ordering is this module's job and nobody else's: `claude-ps` sorts by pid so that two runs a
/// second apart diff cleanly, and says in its README that this order is for diffing rather than
/// reading.
///
/// 🔴 **`luneta`'s order, both halves.** Attention first, by [`State`]'s own `Ord`; and within
/// one status, **most recent first** — the agent that changed a moment ago is the one you were
/// just working with, which is a stabler thing to steer by than the one that has been sitting
/// there longest. ⚠️ That second half is a reversal: this menu used to put the *oldest* row of a
/// group at the top, on the reasoning that it had been ignored longest. The picker's reasoning
/// won because the picker is where he actually navigates from, and two surfaces that disagree
/// about which row is at the top are worse than either rule.
///
/// Ties fall back to the producer's pid order, because `sort_by` is stable — so two agents that
/// changed in the same second do not swap places between polls.
pub fn snapshot(rows: &[Row], now: u64) -> Snapshot {
    let mut entries: Vec<Entry> = rows
        .iter()
        .map(|row| Entry {
            state: classify(&row.raw_status),
            raw_status: row.raw_status.clone(),
            // Decided here, in three steps, because it is a function of this row alone. Only
            // the `:pane` suffix needs the other rows, and that is `name_rows`.
            title: chosen_name(&row.name, row.name_source.as_deref())
                .or_else(|| row.zellij.as_ref().map(|z| z.session.clone()))
                .unwrap_or_else(|| row.name.clone()),
            age_s: row.transition_age_s,
            // One `map`, because the producer nests the pair: there is no state where a
            // session is known and its pane is not, so there is nothing left to agree on here.
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

/// The name a **person** chose for this agent, or `None` to fall back to something else.
///
/// 🔴 `claude-ps` reports both the name and **who chose it**, and the second half is the
/// load-bearing one. A `derived` name is the basename of the cwd plus a suffix, so a row that
/// showed it would be named after a directory it is only incidentally in. Only `user` and `peer`
/// are a name that a person or another agent picked.
///
/// 🔴 An unrecognised source is **suppressed**, which is the exact opposite of what [`classify`]
/// does with an unrecognised status. The asymmetry is the producer's and it is deliberate on both
/// sides: every value in the status vocabulary is a real state, so hiding one hides a live agent,
/// whereas the sources that carry a *chosen* name are a short closed list and the machinery is
/// the long open one — Claude Code already writes `derived`, `collision`, `auto` and `hook`. A
/// source invented tomorrow is far likelier to be more machinery, and trusting it would put a
/// generated name where a chosen one belongs, which reads as information and is not.
///
/// `None` is trusted, because it is the state before the key existed rather than a source this
/// build failed to recognise, and an older `claude-ps` should keep working.
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

/// Spell out the pane on every row whose name has stopped picking one row out of the menu.
///
/// 🔴 [[CSB-15]] named every row by its **zellij session**, because `Row::name` was Claude Code's
/// own label — the cwd basename plus a suffix, a string that appears nowhere in how the session
/// is reached. `name_source` is what put the question back: a name a *person* chose is the only
/// string on the row that says what the agent is **for**, and it is not the cwd twice over. So
/// the name is decided in `snapshot`, in the picker's three steps: the chosen name; else the
/// zellij session; else Claude Code's label, for an agent that has neither and would otherwise
/// have no name at all. What is left for here is the suffix.
///
/// Two parts of it are load-bearing, and both are `luneta`'s:
///
/// - **Ambiguity is judged over the rows that render.** Nothing is hidden from this menu, so
///   that is every entry; the suffix therefore appears exactly when the bare name would leave
///   two rows looking identical.
/// - **When a name is shared, *every* one of its rows is suffixed.** "The first one is bare"
///   is not a rule anyone could read off the menu.
///
/// ⚠️ The suffix needs a pane, so only rows inside zellij can take one. An agent outside it
/// keeps its bare name — `-:-` would be worse than the collision it announced.
/// ⚠️ **Every comparison happens before any rename.** Suffixing in the same pass would have the
/// second row of a pair compare its bare name against the first row's *already suffixed* one,
/// find no collision, and leave exactly the bare name the suffix was there to disambiguate —
/// the "first one is bare" rule above, arrived at by accident.
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

/// Single-unit ages: `<1m`, `47m`, `3h`, `2d`. Precision past the leading unit is noise when
/// the question is only ever "has this been sitting there a while?".
fn humanise(secs: u64) -> String {
    match secs {
        0..60 => "<1m".to_string(),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// Middle-truncate, because 🔴 **the tail has to survive**: a `:pane` suffix is the whole of
/// what tells two rows in one session apart, and the fallback names for agents outside zellij
/// are cwd basenames, where `projeto-ponte-55` and `projeto-ponte-61` differ only at the end.
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

    /// 🔴 `luneta`'s rank, and the reason [`State`]'s variants are declared in this order.
    #[test]
    fn the_states_are_the_pickers_four_in_the_pickers_order() {
        assert_eq!(classify("waiting"), State::Waiting);
        assert_eq!(classify("idle"), State::Idle);
        assert_eq!(classify("busy"), State::Busy);
        assert_eq!(classify("shell"), State::Other);

        assert!(State::Waiting < State::Idle);
        assert!(State::Idle < State::Busy);
        assert!(State::Busy < State::Other);
    }

    /// The picker compares with `eq_ignore_ascii_case`, so this end has to as well or the same
    /// word ranks differently on the two surfaces.
    #[test]
    fn a_status_is_read_case_insensitively() {
        assert_eq!(classify("WAITING"), State::Waiting);
        assert_eq!(classify("Idle"), State::Idle);
    }

    /// The version-skew rule. A status this build has never heard of must land somewhere
    /// harmless, and last-and-uncounted is the only harmless place.
    #[test]
    fn an_unknown_status_ranks_last_and_is_never_counted() {
        for unknown in ["shell", "compacting", "", "nonsense"] {
            let s = classify(unknown);
            assert_eq!(s, State::Other, "{unknown:?}");
            assert!(!s.is_actionable(), "{unknown:?}");
        }
    }

    /// 🔴 `your turn` is gone, and with it the hour-long timeout and the newborn suppression
    /// that propped it up. A finished turn sorts above a working one and is **not** counted, at
    /// every age — including a five-second-old session, which used to be filed under `dormant`
    /// to stop it nagging, and a week-old one, which used to age out of `your turn` to the same
    /// end. Neither needs a threshold now.
    #[test]
    fn an_idle_agent_is_never_counted_at_any_age() {
        for age in [0, 5, 3_600, 7 * 86_400] {
            let snap = snapshot(&[row("idle", age)], NOW);
            assert_eq!(snap.badge, 0, "{age}s");
            assert_eq!(snap.entries[0].state, State::Idle, "{age}s");
        }
    }

    /// The invariant the whole applet carries: the number in the bar is the number of agents
    /// with a question pending.
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

    /// 🔴 `luneta`'s order, both halves — and the second half is a reversal of what this menu
    /// used to do. Attention first; then, within one status, **most recent first**.
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

    /// ⚠️ Ties keep the producer's pid order, so two agents that changed in the same second do
    /// not swap places between one poll and the next.
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

    /// 🔴 The clock moves even when the list does not. The menu is rebuilt from a frozen
    /// snapshot ten times a second while it is open; without the offset every row would read the
    /// age it had when `claude-ps` last ran.
    #[test]
    fn the_age_counts_on_after_the_snapshot_was_taken() {
        let snap = snapshot(&[row("waiting", 59)], NOW);
        assert_eq!(snap.since(NOW), 0);
        assert!(snap.entries[0].label(0, 0).ends_with("<1m"));
        assert_eq!(snap.since(NOW + 90), 90);
        assert!(snap.entries[0].label(0, 90).ends_with("2m"));
    }

    /// ⚠️ A clock that goes backwards stops the ages rather than jumping them.
    #[test]
    fn a_backwards_clock_reads_as_fresh() {
        assert_eq!(snapshot(&[row("idle", 10)], NOW).since(NOW - 500), 0);
    }

    /// The word beside the glyph is the producer's, so a status invented after this was written
    /// says what it is rather than being translated into one of ours.
    #[test]
    fn the_row_prints_the_status_it_was_given() {
        let snap = snapshot(&[row("compacting", 120)], NOW);
        let label = snap.entries[0].label(0, 0);
        assert!(label.contains("compacting"), "{label}");
        assert!(label.starts_with(UNKNOWN_GLYPH), "{label}");
    }

    /// 🔴 A `derived` name is the cwd basename plus a suffix, so the row is named by its
    /// address instead.
    #[test]
    fn a_derived_name_loses_to_the_zellij_session() {
        assert_eq!(snapshot(&[row("idle", 5)], NOW).entries[0].title, "s");
    }

    /// 🔴 …and a name a person chose wins, because it is the only string on the row that says
    /// what the agent is for.
    #[test]
    fn a_chosen_name_wins_over_the_session() {
        for source in ["user", "peer", "USER"] {
            let mut r = row("idle", 5);
            r.name_source = Some(source.into());
            assert_eq!(snapshot(&[r], NOW).entries[0].title, "n", "{source}");
        }
    }

    /// An absent source is the state before Claude Code recorded one, and is trusted; a source
    /// this build does not know is suppressed, because the machinery is the long open list.
    #[test]
    fn an_absent_source_is_trusted_and_an_unknown_one_is_not() {
        let mut absent = row("idle", 5);
        absent.name_source = None;
        assert_eq!(snapshot(&[absent], NOW).entries[0].title, "n");

        let mut unknown = row("idle", 5);
        unknown.name_source = Some("something-new".into());
        assert_eq!(snapshot(&[unknown], NOW).entries[0].title, "s");
    }

    /// An agent outside zellij has no session to be named by, so Claude Code's label is the only
    /// identifier it has left — derived or not.
    #[test]
    fn an_agent_outside_zellij_keeps_the_label_it_has() {
        let mut r = row("idle", 5);
        r.zellij = None;
        let snap = snapshot(&[r], NOW);
        assert_eq!(snap.entries[0].title, "n");
        assert_eq!(snap.entries[0].target, None);
    }

    /// 🔴 When a name is shared, *every* one of its rows is suffixed — "the first one is bare"
    /// is not a rule anyone could read off the menu.
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

    /// ⚠️ …and a name that still picks one row out is left alone.
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

    /// Only the busy row's glyph moves, and it is the only reason a tick is worth a repaint.
    #[test]
    fn only_busy_spins() {
        let busy = snapshot(&[row("busy", 5)], NOW);
        assert!(busy.any_spinning());
        assert_ne!(busy.entries[0].glyph(0), busy.entries[0].glyph(1));

        let idle = snapshot(&[row("idle", 5)], NOW);
        assert!(!idle.any_spinning());
        assert_eq!(idle.entries[0].glyph(0), idle.entries[0].glyph(7));
    }

    /// Calm is the bare mark, not a `0` — a zero is still something to read.
    #[test]
    fn a_quiet_badge_is_empty_rather_than_zero() {
        assert_eq!(snapshot(&[row("idle", 5)], NOW).badge_text(), "");
        assert_eq!(snapshot(&[row("waiting", 5)], NOW).badge_text(), "1");
    }

    /// 🔴 The tail has to survive: it is where a `:pane` suffix lives and where two cwd-derived
    /// names differ.
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
