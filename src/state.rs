//! The mapping from what `claude-agents` says to what the tray shows.
//!
//! 🔴 **This is the one place the mapping lives.** `claude-agents` passes `status` through
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

    /// Echoes the tray glyph vocabulary, so the shape in the bar is visibly the shape of the
    /// rows that caused it.
    pub fn glyph(self) -> char {
        match self {
            State::NeedsInput => '\u{25C8}', // ◈
            State::YourTurn => '\u{25C6}',   // ◆
            State::Working => '\u{25CB}',    // ○
            State::Dormant => '\u{00B7}',    // ·
        }
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
    /// What this row calls the agent — its **zellij session name**, which is the string
    /// Lorenzo navigates by. Filled in by `name_rows`, because a session name can only be
    /// judged sufficient against the other rows in the same menu.
    pub title: String,
    pub age_s: u64,
    pub target: Option<Target>,
}

impl Entry {
    /// [[CSB-2]]'s row: `glyph · title · state age`.
    ///
    /// `host` is gone — one machine in this slice. `cwd` never renders. **Every** row carries
    /// an age, working rows included: on a working row the age is turn duration, and it is the
    /// only place a wedged session becomes visible at all.
    pub fn label(&self) -> String {
        format!(
            "{}  {:<width$}  {} {}",
            self.state.glyph(),
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

    match row.raw_status.as_str() {
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
/// Ordering is this module's job and nobody else's: `claude-agents` sorts by session, pane and
/// pid so that two runs a second apart diff cleanly, and says in its README that this order is
/// for diffing rather than reading. **Actionable first, then oldest first** — within a group,
/// the row that has been waiting longest is the one that has been ignored longest.
pub fn snapshot(rows: &[Row], now: u64) -> Snapshot {
    let mut entries: Vec<Entry> = rows
        .iter()
        .map(|row| Entry {
            state: classify(row, now),
            // Provisional. `name_rows` overwrites this with the zellij session; Claude Code's
            // own label survives only where there is no session to use instead.
            title: row.name.clone(),
            age_s: row.transition_age_s,
            target: (row.session != "-" && row.pane != "-").then(|| Target {
                session: row.session.clone(),
                pane: row.pane.clone(),
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

    const NOW: u64 = 1_800_000_000;

    fn row(status: &str, transition_age_s: u64, since_start: u64) -> Row {
        Row {
            raw_status: status.into(),
            transition_age_s,
            session: "s".into(),
            pane: "0".into(),
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
        for unknown in ["busy", "shell", "compacting", "", "IDLE"] {
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
        r.session = "-".into();
        r.pane = "-".into();
        assert_eq!(snapshot(&[r], NOW).entries[0].target, None);
    }

    fn agent(session: &str, pane: &str, name: &str, age: u64) -> Row {
        let mut r = row("idle", age, 900);
        r.session = session.into();
        r.pane = pane.into();
        r.name = name.into();
        r
    }

    /// 🔴 [[CSB-15]], as a regression test. `master-3c` is Claude Code's own label for an
    /// agent in `…/infra.git/master`; the session it lives in is `infra`. Naming the row by the
    /// label put a string on screen that Lorenzo has no way to connect to a session.
    #[test]
    fn a_row_is_named_by_its_zellij_session_not_by_claude_code_s_label() {
        let snap = snapshot(&[agent("infra", "1", "master-3c", 60)], NOW);
        assert_eq!(snap.entries[0].title, "infra");
        assert!(snap.entries[0].label().contains("infra"));
        assert!(
            !snap.entries[0].label().contains("master-3c"),
            "the cwd basename is not an address"
        );
    }

    /// `zj-picker`'s rule, copied: the pane is spelled out exactly when the bare session name
    /// has stopped picking one row out — and then on **both** rows, because "the first one is
    /// bare" is not a rule anyone could read off the menu.
    #[test]
    fn two_agents_in_one_session_both_spell_out_the_pane() {
        let rows = [
            agent("infra", "1", "master-3c", 60),
            agent("infra", "2", "hotfix-7a", 30),
            agent("nixos", "0", "nixos-69", 20),
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
        let mut waiting = agent("infra", "1", "master-3c", 10);
        waiting.raw_status = "waiting".into();
        let rows = [waiting, agent("infra", "2", "hotfix-7a", 99_999)];
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
            agent("-", "-", "projeto-ponte-55", 60),
            agent("-", "-", "projeto-ponte-61", 30),
        ];
        let titles: Vec<String> = snapshot(&rows, NOW)
            .entries
            .iter()
            .map(|e| e.title.clone())
            .collect();
        assert_eq!(titles, vec!["projeto-ponte-55", "projeto-ponte-61"]);
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
