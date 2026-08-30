//! The mapping from what `claude-ps` says to what the tray shows.
//!
//! 🔴 **This is the one place the mapping lives.** `claude-ps` passes `status` through
//! verbatim and tells consumers not to match it against a fixed set, so somebody downstream has
//! to decide — and it has to be the same somebody that draws the badge and builds the menu, or
//! the count and the list can disagree about the same session.
//!
//! Everything here is pure and takes `now` as an argument, so every rule below is a test rather
//! than a thing you have to run the applet to see.

use crate::agents::Row;

/// A *your turn* row stops nagging after an hour.
///
/// 🔴 Deliberately generous. A badge reading `0` while something waits is a strictly worse
/// failure than one that over-counts, so the threshold errs long.
const FINISHED_AFTER_S: u64 = 3600;

/// A session that has only just started is not asking for anything yet.
///
/// Without this, opening a tab nags immediately: a fresh agent is `idle`, and `idle` is how
/// *finished* looks.
const NEWBORN_S: u64 = 30;

/// Names middle-truncate here. ⚠️ The columns line up only because the GTK menu font on this
/// box happens to be monospace — a system setting, not a guarantee.
const NAME_WIDTH: usize = 28;

/// One turn of the busy spinner, a frame per animation tick.
///
/// Braille rather than the ASCII `|/-\`: every frame here is **one column wide**, so the glyph
/// column keeps its width as the spinner turns rather than shoving the name column back and
/// forth ten times a second for the whole time an agent is busy.
///
/// 🔴 **These are not `zj-picker`'s frames — the one place this end's picture deliberately
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
/// ⚠️ What stays shared with `zj-picker` is the half that carries meaning: **which status spins,
/// and that nothing else does**. Somebody who reads the picker reads this menu right; the frames
/// are heavier, not different. The cycle is eight frames where the picker's is ten, so a turn
/// takes 0.8 s at `crate::TICK` rather than a round second — a spinner has no speed anyone reads.
///
/// ⚠️ Each frame carries a **trailing space** and the emoji beside it do not. That is the whole
/// of how this column stays aligned: an emoji is two columns wide where a braille cell is one,
/// so the space is the second column the spinner would otherwise be missing. `zj-picker`
/// measures its tag column with `unicode-width`; a dependency to measure five known strings is
/// not the trade here, and it is why the padding lives *in the table* rather than in
/// [`Entry::label`] — the format string below cannot tell the two widths apart.
const SPINNER: [&str; 8] = ["⣾ ", "⣽ ", "⣻ ", "⢿ ", "⡿ ", "⣟ ", "⣯ ", "⣷ "];

/// Unidentified, and deliberately not one of the four — a status this build cannot name must
/// not be able to pass itself off as one it can. `zj-picker`'s, like the rest of the table.
const UNKNOWN_GLYPH: &str = "🛸";

/// What the applet believes about one agent.
///
/// Four states, two of them counted. The split is not "urgent vs not" — it is **does this row
/// want something from Lorenzo**, which is the invariant the badge exists to carry:
/// `badge > 0 ⟺ there is something to do`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// Blocked on him. 🔴 **Never ages out** — a permission prompt will not answer itself.
    NeedsInput,
    /// Finished its turn recently. Ages out at [`FINISHED_AFTER_S`].
    YourTurn,
    /// Busy, or a status this build does not recognise. Listed, never counted.
    Working,
    /// Idle and old, or idle and newborn. 🔴 **Listed, dimmed, uncounted — not hidden.**
    /// Ageing out means *stops nagging*, not *is lost*.
    Dormant,
}

impl State {
    /// The badge is `count(actionable)`, and this is what actionable means.
    pub fn is_actionable(self) -> bool {
        matches!(self, State::NeedsInput | State::YourTurn)
    }

    pub fn label(self) -> &'static str {
        match self {
            State::NeedsInput => "needs input",
            State::YourTurn => "your turn",
            State::Working => "working",
            State::Dormant => "idle",
        }
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
    /// Verbatim from the producer, and what [`Entry::glyph`] draws the row's picture from.
    ///
    /// 🔴 **It decides the picture and nothing else.** Whether a row is counted, where it
    /// sorts, and what word appears beside it are all [`State`]'s, via `classify` — the two
    /// read the same word by the same rule and reach different questions, which is what keeps
    /// `zj-picker`'s vocabulary and this applet's judgment from becoming two mappings.
    pub raw_status: String,
    /// What this row calls the agent — its **zellij session name**, which is the string
    /// Lorenzo navigates by. Filled in by `name_rows`, because a session name can only be
    /// judged sufficient against the other rows in the same menu.
    pub title: String,
    pub age_s: u64,
    pub target: Option<Target>,
}

impl Entry {
    /// The picture on this row.
    ///
    /// 🔴 **Keyed by the producer's word, and `zj-picker`'s agents tab is the standard.** Not by
    /// [`State`] — which is tempting, because the states are what this applet actually believes
    /// and they say things no status word does, but it would mean the same agent wore two
    /// different faces depending on which surface it was read on. One vocabulary, one meaning
    /// per picture, and this end of it does not get a vote. The table below is `zj-picker`'s
    /// `agents::glyph`, case-insensitivity included — [`SPINNER`]'s frames are the single
    /// exception, and they are heavier rather than other. [`classify`] reads the same word the
    /// same way, so there is no status that can take one module's answer and the other's picture.
    ///
    /// ⚠️ So the *state* is not in the glyph at all — a `your turn` row and a `dormant` one are
    /// both ☕, and what tells them apart is the word beside them and the block they sit in.
    /// That is `zj-picker`'s bargain too: over there `idle` is one row whether it finished a
    /// minute ago or yesterday, and the age column is what says which.
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
            // which is why it gets a cup rather than the "do not disturb" of a 💤.
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

    /// [[CSB-2]]'s row: `glyph · title · state age`.
    ///
    /// `host` is gone — one machine in this slice. `cwd` never renders. **Every** row carries
    /// an age, working rows included: on a working row the age is turn duration, and it is the
    /// only place a wedged session becomes visible at all.
    pub fn label(&self, frame: u64) -> String {
        format!(
            "{}  {:<width$}  {} {}",
            self.glyph(frame),
            truncate(&self.title, NAME_WIDTH),
            self.state.label(),
            humanise(self.age_s),
            width = NAME_WIDTH,
        )
    }
}

/// Everything the tray needs for one repaint, derived once so the badge and the list cannot
/// disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    /// `count(actionable)`.
    pub badge: usize,
    /// At least one row is *blocked* rather than merely finished — which is what earns the
    /// fourth glyph, so "something is stuck" reads without opening anything.
    pub blocked: bool,
}

impl Snapshot {
    /// What the pixmap spells beside the mark. **Empty when there is nothing to do** — calm is
    /// the bare mark, not a `0`, because a zero is still something to read.
    ///
    /// 🔴 This used to prefix a glyph (`◇`/`◆ 3`/`◈ 2`). The mark took that slot, so the
    /// blocked-vs-merely-finished distinction moved into the *colour* of this count — see
    /// [`Snapshot::blocked`] and `crate::icon::BLOCKED`. Nothing was dropped, only recoloured.
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

    pub fn iter(&self, state: State) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| e.state == state)
    }
}

/// Classify one row.
///
/// 🔴 The last arm is the load-bearing one: **anything unrecognised is `Working`, never
/// actionable.** Version skew reaches the key set on this box — `pidDomain` exists on 2.1.251
/// records and not on 2.1.241 — so a status nobody has seen yet is a matter of time. Failing to
/// `Working` fails silent; failing to actionable would invent a badge out of a typo.
fn classify(row: &Row, now: u64) -> State {
    let since_start = now.saturating_sub(row.started_at);

    // 🔴 Lowercased because `zj-picker` lowercases — it compares every status with
    // `eq_ignore_ascii_case`, and its table is the standard for what a status *is*. Matching
    // exactly here instead would have let a hypothetical `Idle` be a counted row on one surface
    // and an unrecognised one on the other, and it errs the wrong way besides: an unrecognised
    // row is never counted, so a badge reading 0 while something waits is exactly the failure
    // `FINISHED_AFTER_S` is generous to avoid.
    match row.raw_status.to_ascii_lowercase().as_str() {
        // 🔴 Never ages. Asymmetric with `idle` on purpose — do not tidy the two thresholds
        // into one. A blocked session does not resolve itself by being ignored.
        "waiting" => State::NeedsInput,

        // A brand new agent is `idle`, and `idle` is also how *finished* looks. Without this
        // arm, opening a tab nags you about the tab you just opened.
        "idle" if since_start < NEWBORN_S => State::Dormant,
        "idle" if row.transition_age_s < FINISHED_AFTER_S => State::YourTurn,
        "idle" => State::Dormant,

        _ => State::Working,
    }
}

/// Turn a poll into a repaint.
///
/// Ordering is this module's job and nobody else's: `claude-ps` sorts by session, pane and
/// pid so that two runs a second apart diff cleanly, and says in its README that this order is
/// for diffing rather than reading. **Actionable first, then oldest first** — within a group,
/// the row that has been waiting longest is the one that has been ignored longest.
pub fn snapshot(rows: &[Row], now: u64) -> Snapshot {
    let mut entries: Vec<Entry> = rows
        .iter()
        .map(|row| Entry {
            state: classify(row, now),
            raw_status: row.raw_status.clone(),
            // Provisional. `name_rows` overwrites this with the zellij session; Claude Code's
            // own label survives only where there is no session to use instead.
            title: row.name.clone(),
            age_s: row.transition_age_s,
            // One `map`, because the producer nests the pair: there is no state where a
            // session is known and its pane is not, so there is nothing left to agree on here.
            target: row.zellij.as_ref().map(|z| Target {
                session: z.session.clone(),
                pane: z.pane.clone(),
            }),
        })
        .collect();

    entries.sort_by(|a, b| a.state.cmp(&b.state).then(b.age_s.cmp(&a.age_s)));
    name_rows(&mut entries);

    let badge = entries.iter().filter(|e| e.state.is_actionable()).count();
    let blocked = entries.iter().any(|e| e.state == State::NeedsInput);

    Snapshot {
        entries,
        badge,
        blocked,
    }
}

/// Name every row by its **zellij session**, and spell out the pane only when that name has
/// stopped picking one row out of the menu.
///
/// 🔴 [[CSB-15]]. Rows used to be named by `Row::name` — Claude Code's own label, which is
/// the cwd basename plus a two-character suffix. For `…/infra.git/master` that label is
/// `master-3c`, a string that appears nowhere in how the session is reached: the zellij session
/// is `infra`. The two were conflated as "the session name"; they are different things, and
/// only one of them is an address.
///
/// The rule is `zj-picker`'s, deliberately copied rather than reinvented — it is the surface
/// Lorenzo asked this one to resemble. Two parts of it are load-bearing:
///
/// - **Ambiguity is judged over the rows that render.** Nothing is hidden from this menu, so
///   that is every entry; the suffix therefore appears exactly when the bare name would leave
///   two rows looking identical.
/// - **When a session is shared, *every* one of its rows is suffixed.** "The first one is bare"
///   is not a rule anyone could read off the menu.
///
/// An agent outside zellij has no session to be named by, keeps Claude Code's label as the only
/// identifier it has, and is never suffixed — `-:-` would be worse than the name it replaced.
fn name_rows(entries: &mut [Entry]) {
    for i in 0..entries.len() {
        let Some(target) = entries[i].target.clone() else {
            continue;
        };
        let shared = entries.iter().enumerate().any(|(j, other)| {
            j != i
                && other
                    .target
                    .as_ref()
                    .is_some_and(|o| o.session == target.session)
        });
        entries[i].title = if shared {
            format!("{}:{}", target.session, target.pane)
        } else {
            target.session
        };
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

    fn row(status: &str, transition_age_s: u64, since_start: u64) -> Row {
        Row {
            raw_status: status.into(),
            transition_age_s,
            zellij: Some(Zellij {
                session: "s".into(),
                pane: "0".into(),
            }),
            name: "n".into(),
            started_at: NOW - since_start,
        }
    }

    #[test]
    fn waiting_is_needs_input() {
        assert_eq!(classify(&row("waiting", 5, 900), NOW), State::NeedsInput);
    }

    /// 🔴 The asymmetry, stated as a test so it survives a tidy-up. `your turn` ages out at an
    /// hour; `needs input` never does, because nothing about being ignored unblocks it.
    #[test]
    fn needs_input_never_ages_but_your_turn_does() {
        let week = 7 * 86_400;
        assert_eq!(
            classify(&row("waiting", week, week), NOW),
            State::NeedsInput
        );
        assert_eq!(
            classify(&row("idle", FINISHED_AFTER_S - 1, week), NOW),
            State::YourTurn
        );
        assert_eq!(
            classify(&row("idle", FINISHED_AFTER_S, week), NOW),
            State::Dormant
        );
    }

    /// Opening a tab must not nag you about the tab you just opened.
    #[test]
    fn a_newborn_idle_session_is_dormant() {
        assert_eq!(classify(&row("idle", 0, 5), NOW), State::Dormant);
        assert_eq!(
            classify(&row("idle", 0, NEWBORN_S + 1), NOW),
            State::YourTurn
        );
    }

    /// The version-skew rule. A status this build has never heard of must land somewhere
    /// harmless, and `Working` is the only harmless place.
    #[test]
    fn an_unknown_status_is_working_and_never_counted() {
        for unknown in ["busy", "shell", "compacting", "", "nonsense"] {
            let s = classify(&row(unknown, 10, 900), NOW);
            assert_eq!(s, State::Working, "{unknown:?}");
            assert!(!s.is_actionable(), "{unknown:?}");
        }
    }

    /// The invariant the whole applet exists to carry.
    #[test]
    fn badge_counts_exactly_the_actionable_rows() {
        let rows = [
            row("waiting", 10, 900),
            row("idle", 60, 900),
            row("busy", 60, 900),
            row("idle", 99_999, 99_999),
        ];
        let snap = snapshot(&rows, NOW);
        assert_eq!(snap.badge, 2);
        assert!(snap.blocked);
        assert_eq!(snap.entries.len(), 4, "nothing is hidden, only uncounted");
    }

    /// Dormant rows stay in the list. Ageing out means *stops nagging*, not *is lost* — the
    /// exact reason an hour-long threshold is safe.
    #[test]
    fn dormant_rows_are_listed_not_dropped() {
        let snap = snapshot(&[row("idle", 99_999, 99_999)], NOW);
        assert_eq!(snap.badge, 0);
        assert_eq!(snap.iter(State::Dormant).count(), 1);
        assert_eq!(
            snap.badge_text(),
            "",
            "a dormant row is uncounted, so the mark stands alone"
        );
    }

    #[test]
    fn actionable_first_then_oldest_first() {
        let rows = [
            row("idle", 100, 900),
            row("busy", 5, 900),
            row("waiting", 50, 900),
            row("idle", 200, 900),
        ];
        let states: Vec<_> = snapshot(&rows, NOW)
            .entries
            .iter()
            .map(|e| (e.state, e.age_s))
            .collect();
        assert_eq!(
            states,
            vec![
                (State::NeedsInput, 50),
                (State::YourTurn, 200),
                (State::YourTurn, 100),
                (State::Working, 5),
            ]
        );
    }

    /// 🔴 Calm spells **nothing**, not `0`. The mark alone is the calm state; a zero beside it
    /// is one more number in a bar full of them, and it would have to be read to be dismissed.
    #[test]
    fn calm_spells_nothing_and_otherwise_the_count() {
        assert_eq!(snapshot(&[], NOW).badge_text(), "");
        assert_eq!(snapshot(&[row("busy", 60, 900)], NOW).badge_text(), "");
        assert_eq!(snapshot(&[row("idle", 60, 900)], NOW).badge_text(), "1");
        assert_eq!(
            snapshot(&[row("waiting", 60, 900), row("idle", 60, 900)], NOW).badge_text(),
            "2"
        );
    }

    /// *Blocked* rather than merely *finished* is the difference between "nothing is moving"
    /// and "come back when you can". The count reads the same either way, so `blocked` is what
    /// the icon colours it by — losing this flag would silently flatten the two.
    #[test]
    fn blocked_is_flagged_separately_from_the_count() {
        assert!(!snapshot(&[row("idle", 60, 900)], NOW).blocked);
        assert!(snapshot(&[row("waiting", 60, 900), row("idle", 60, 900)], NOW).blocked);
    }

    #[test]
    fn an_agent_outside_zellij_has_nowhere_to_jump() {
        let mut r = row("waiting", 10, 900);
        r.zellij = None;
        assert_eq!(snapshot(&[r], NOW).entries[0].target, None);
    }

    fn agent(zellij: Option<(&str, &str)>, name: &str, age: u64) -> Row {
        let mut r = row("idle", age, 900);
        r.zellij = zellij.map(|(session, pane)| Zellij {
            session: session.into(),
            pane: pane.into(),
        });
        r.name = name.into();
        r
    }

    /// 🔴 [[CSB-15]], as a regression test. `master-3c` is Claude Code's own label for an
    /// agent in `…/infra.git/master`; the session it lives in is `infra`. Naming the row by the
    /// label put a string on screen that Lorenzo has no way to connect to a session.
    #[test]
    fn a_row_is_named_by_its_zellij_session_not_by_claude_code_s_label() {
        let snap = snapshot(&[agent(Some(("infra", "1")), "master-3c", 60)], NOW);
        assert_eq!(snap.entries[0].title, "infra");
        assert!(snap.entries[0].label(0).contains("infra"));
        assert!(
            !snap.entries[0].label(0).contains("master-3c"),
            "the cwd basename is not an address"
        );
    }

    /// `zj-picker`'s rule, copied: the pane is spelled out exactly when the bare session name
    /// has stopped picking one row out — and then on **both** rows, because "the first one is
    /// bare" is not a rule anyone could read off the menu.
    #[test]
    fn two_agents_in_one_session_both_spell_out_the_pane() {
        let rows = [
            agent(Some(("infra", "1")), "master-3c", 60),
            agent(Some(("infra", "2")), "hotfix-7a", 30),
            agent(Some(("nixos", "0")), "nixos-69", 20),
        ];
        let titles: Vec<String> = snapshot(&rows, NOW)
            .entries
            .iter()
            .map(|e| e.title.clone())
            .collect();
        assert_eq!(titles, vec!["infra:1", "infra:2", "nixos"]);
    }

    /// The suffix is judged against every row in the menu, not against rows of the same state —
    /// a dormant row and a waiting one that share a session are still two rows reading `infra`.
    #[test]
    fn sharing_is_judged_across_the_whole_menu_not_within_one_state() {
        let mut waiting = agent(Some(("infra", "1")), "master-3c", 10);
        waiting.raw_status = "waiting".into();
        let rows = [waiting, agent(Some(("infra", "2")), "hotfix-7a", 99_999)];
        let snap = snapshot(&rows, NOW);
        assert_eq!(snap.entries[0].state, State::NeedsInput);
        assert_eq!(snap.entries[1].state, State::Dormant);
        assert_eq!(snap.entries[0].title, "infra:1");
        assert_eq!(snap.entries[1].title, "infra:2");
    }

    /// Nowhere to jump means nothing to be named by, so Claude Code's label is all there is —
    /// and two such rows are never suffixed, because `-:-` says less than the label does.
    #[test]
    fn an_agent_outside_zellij_keeps_claude_code_s_label() {
        let rows = [
            agent(None, "projeto-ponte-55", 60),
            agent(None, "projeto-ponte-61", 30),
        ];
        let titles: Vec<String> = snapshot(&rows, NOW)
            .entries
            .iter()
            .map(|e| e.title.clone())
            .collect();
        assert_eq!(titles, vec!["projeto-ponte-55", "projeto-ponte-61"]);
    }

    /// 🔴 Case is `zj-picker`'s rule, not this module's: it compares every status with
    /// `eq_ignore_ascii_case`, so `classify` does too. The alternative was a status that is
    /// *recognised* on one surface and *unknown* on the other, which is two mappings again —
    /// and the unknown side is the uncounted one, so the skew would hide a row rather than
    /// merely mislabel it.
    #[test]
    fn case_never_decides_what_a_status_is() {
        for waiting in ["waiting", "WAITING", "Waiting"] {
            assert_eq!(
                classify(&row(waiting, 5, 900), NOW),
                State::NeedsInput,
                "{waiting}"
            );
        }
        for idle in ["idle", "IDLE", "Idle"] {
            assert_eq!(
                classify(&row(idle, 60, 900), NOW),
                State::YourTurn,
                "{idle}"
            );
        }
        assert_eq!(
            entry(State::Working, "BUSY").glyph(0),
            entry(State::Working, "busy").glyph(0)
        );
    }

    fn entry(state: State, raw_status: &str) -> Entry {
        Entry {
            state,
            raw_status: raw_status.into(),
            title: "infra".into(),
            age_s: 60,
            target: None,
        }
    }

    /// 🔴 `zj-picker`'s table — one vocabulary across both surfaces, so an agent read in the
    /// picker and the same agent read in the tray are not two different pictures. The busy row
    /// is checked against [`SPINNER`] rather than a literal: its frames are this end's own, for
    /// the reason on that constant, and what has to hold is that `busy` is the status that spins.
    #[test]
    fn the_glyphs_are_zj_pickers_and_nothing_is_added() {
        assert_eq!(entry(State::NeedsInput, "waiting").glyph(0), "\u{1f64b}");
        assert_eq!(entry(State::YourTurn, "idle").glyph(0), "\u{2615}");
        assert_eq!(entry(State::Working, "shell").glyph(0), "\u{1f41a}");
        assert_eq!(entry(State::Working, "compacting").glyph(0), UNKNOWN_GLYPH);
        assert!(SPINNER.contains(&entry(State::Working, "busy").glyph(0)));
    }

    /// ⚠️ The state is **not** in the glyph. A row that finished a minute ago and one that aged
    /// out an hour ago are both `idle` to the producer and both ☕ here; the word beside them and
    /// the block they sit in are what tell them apart. That is the cost of one vocabulary, and
    /// it is `zj-picker`'s bargain as much as this one's.
    #[test]
    fn the_state_does_not_change_the_picture() {
        for state in [
            State::NeedsInput,
            State::YourTurn,
            State::Working,
            State::Dormant,
        ] {
            assert_eq!(entry(state, "idle").glyph(0), "\u{2615}", "{state:?}");
        }
        let fresh = snapshot(&[row("idle", 60, 900)], NOW);
        let aged = snapshot(&[row("idle", 99_999, 99_999)], NOW);
        assert_eq!(fresh.entries[0].state, State::YourTurn);
        assert_eq!(aged.entries[0].state, State::Dormant);
        assert_eq!(fresh.entries[0].glyph(0), aged.entries[0].glyph(0));
        assert_ne!(fresh.entries[0].label(0), aged.entries[0].label(0));
    }

    /// The one glyph that moves, and the only one that may — everything else has to be a pure
    /// function of the row, or a repaint on an animation tick would redraw the whole menu.
    #[test]
    fn only_the_busy_glyph_turns() {
        let busy = entry(State::Working, "busy");
        let cycle: Vec<&str> = (0..SPINNER.len() as u64).map(|f| busy.glyph(f)).collect();
        let distinct: std::collections::HashSet<&&str> = cycle.iter().collect();
        assert_eq!(distinct.len(), SPINNER.len(), "every frame is its own");
        assert_eq!(
            busy.glyph(0),
            busy.glyph(SPINNER.len() as u64),
            "the cycle closes"
        );

        for still in ["waiting", "idle", "shell", "compacting"] {
            let e = entry(State::Working, still);
            assert_eq!(e.glyph(0), e.glyph(7), "{still}");
        }
    }

    /// ⚠️ The column lines up only if every glyph is the same *width*, and they are not the same
    /// *length*: an emoji is one char and two columns, a braille cell is one of each. The
    /// spinner's trailing space is the whole of what closes that gap — this is the test that
    /// notices when someone tidies it away.
    #[test]
    fn a_spinner_frame_carries_the_column_an_emoji_gets_for_free() {
        for frame in SPINNER {
            assert!(frame.ends_with(' '), "{frame:?}");
            assert_eq!(frame.chars().count(), 2, "{frame:?}");
        }
        for status in ["waiting", "idle", "shell", "compacting"] {
            let emoji = entry(State::Working, status).glyph(0);
            assert_eq!(emoji.chars().count(), 1, "{emoji:?} pads itself");
        }
    }

    /// What the tray asks before deciding a tick is worth a repaint. A shell and an unknown are
    /// `Working` too, and neither of them moves.
    #[test]
    fn only_a_busy_row_is_a_reason_to_repaint() {
        let still = [
            row("idle", 60, 900),
            row("shell", 60, 900),
            row("waiting", 60, 900),
            row("compacting", 60, 900),
            row("idle", 99_999, 99_999),
        ];
        assert!(!snapshot(&still, NOW).any_spinning());
        assert!(snapshot(&[row("busy", 60, 900)], NOW).any_spinning());
    }

    #[test]
    fn ages_read_in_one_unit() {
        assert_eq!(humanise(0), "<1m");
        assert_eq!(humanise(59), "<1m");
        assert_eq!(humanise(60), "1m");
        assert_eq!(humanise(2_820), "47m");
        assert_eq!(humanise(3_600), "1h");
        assert_eq!(humanise(86_400), "1d");
    }

    #[test]
    fn truncation_keeps_the_suffix_that_tells_two_worktrees_apart() {
        assert_eq!(truncate("short", 28), "short");
        let a = truncate("projeto-ponte-with-a-very-long-name-55", 28);
        let b = truncate("projeto-ponte-with-a-very-long-name-61", 28);
        assert_eq!(a.chars().count(), 28);
        assert_ne!(a, b);
        assert!(a.ends_with("55") && b.ends_with("61"));
    }
}
