//! The one thing this applet asks the outside world: *what is running right now?*
//!
//! 🔴 **This shells out to `claude-ps`; it never reads `~/.claude/sessions` itself.**
//! That program already does the pid + `procStart` liveness check that keeps a recycled pid
//! from passing a dead agent off as live, and already joins each agent to its zellij session
//! and pane. Re-deriving any of that here would create a second source that can disagree with
//! the first — and `zj-picker` is already the second consumer of the first.
//!
//! What comes back is deliberately *raw*. `claude-ps` passes `status` through untouched and
//! its README tells consumers not to match it against a fixed set. Interpreting it is
//! [`crate::state`]'s job, not this module's. Everything here stays at the level of "nine
//! TAB-separated columns arrived, here they are".

use std::process::Command;

/// The program is looked up on `PATH` rather than pinned to a store path, so `claude-ps`
/// can be upgraded underneath the applet without rebuilding it.
const PRODUCER: &str = "claude-ps";

/// The column count as of `claude-ps` 0.1.0 with `started_at`.
///
/// ⚠️ Checked exactly, not as a minimum. `zj-picker` learned this the hard way: a positional
/// parse that tolerates extra columns keeps running against a schema it no longer understands.
/// A mismatch here is a loud [`Error::Columns`], which reaches the tray as `⊘`.
const FIELDS: usize = 9;

/// One line of `claude-ps` output, still uninterpreted.
///
/// Only the fields this applet reads are kept. `pid`, `session_id` and `cwd` are parsed for the
/// arity check and then dropped.
///
/// 🔴 **`name` and `session` are two different things, and calling both of them "the session
/// name" is what [[CSB-15]] cost.** `name` is Claude Code's own label — the cwd basename plus a
/// two-character suffix, so `…/infra.git/master` becomes `master-3c`. `session` is the zellij
/// session that agent is sitting in, `infra`. Only the second is an address, and the first can
/// be unrelated to it. [[CSB-2]] justified dropping `cwd` on the grounds that "the session name
/// already *is* its basename", which is true of `name` and false of `session`; rows are named by
/// `session` now, and `name` survives only as the fallback for an agent outside zellij.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Verbatim from the producer. `busy | idle | waiting | shell`, or anything a future
    /// Claude Code version invents.
    pub raw_status: String,
    /// Seconds in the *current* status.
    ///
    /// ⚠️ Not time since the session started, and it does **not** advance during a busy turn —
    /// `statusUpdatedAt` marks entry into a state. On a working row this reads as turn
    /// duration, which is the only surface a wedged session shows up on.
    pub transition_age_s: u64,
    /// zellij session name, or `-` when the agent is not inside zellij.
    pub session: String,
    /// `$ZELLIJ_PANE_ID`, or `-`. This is exactly what `zellij action focus-pane-id` takes.
    pub pane: String,
    /// Claude Code's own label for the session, derived by it from the cwd basename.
    ///
    /// ⚠️ **Not what a row is named by** — see the note above, and `state::name_rows`.
    /// It renders only when `session` is `-`, where it is the sole identifier left.
    pub name: String,
    /// Wall-clock unix seconds at session start.
    ///
    /// ⚠️ This is `startedAt`, not `procStart` — the latter is jiffies-since-boot and is
    /// meaningless in arithmetic against a timestamp. Newborn suppression needs this one.
    pub started_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `claude-ps` is not on `PATH`, or could not be executed.
    NotFound,
    /// It ran and exited non-zero.
    Failed { code: Option<i32> },
    /// It ran, exited zero, and emitted a line this build cannot read positionally.
    Columns { got: usize, line: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "{PRODUCER} not found on PATH"),
            Error::Failed { code: Some(c) } => write!(f, "{PRODUCER} exited {c}"),
            Error::Failed { code: None } => write!(f, "{PRODUCER} was killed"),
            Error::Columns { got, line } => {
                write!(
                    f,
                    "{PRODUCER} line {line}: {got} columns, expected {FIELDS}"
                )
            }
        }
    }
}

/// Run the producer once and parse what it says.
pub fn poll() -> Result<Vec<Row>, Error> {
    let out = Command::new(PRODUCER)
        .output()
        .map_err(|_| Error::NotFound)?;

    if !out.status.success() {
        return Err(Error::Failed {
            code: out.status.code(),
        });
    }

    parse(&String::from_utf8_lossy(&out.stdout))
}

/// Split the TSV. Pure, so the column contract can be tested without a process.
pub fn parse(stdout: &str) -> Result<Vec<Row>, Error> {
    stdout
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, line)| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() != FIELDS {
                return Err(Error::Columns {
                    got: f.len(),
                    line: i + 1,
                });
            }
            Ok(Row {
                raw_status: f[0].to_string(),
                // A field that will not parse is treated as zero rather than as a failure.
                // The producer owns these numbers; a garbled one costs a wrong age, and
                // throwing the whole row away would cost a session that is really there.
                transition_age_s: f[1].parse().unwrap_or(0),
                session: f[2].to_string(),
                pane: f[3].to_string(),
                name: f[4].to_string(),
                started_at: f[7].parse().unwrap_or(0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str =
        "idle\t14493\tbipa\t0\tprojeto-ponte-55\t3134390\tsome-uuid\t1787965062\t/home/lorenzo/p";

    #[test]
    fn reads_the_nine_columns() {
        let rows = parse(LINE).unwrap();
        assert_eq!(
            rows,
            vec![Row {
                raw_status: "idle".into(),
                transition_age_s: 14493,
                session: "bipa".into(),
                pane: "0".into(),
                name: "projeto-ponte-55".into(),
                started_at: 1787965062,
            }]
        );
    }

    #[test]
    fn blank_lines_are_not_rows() {
        let input = format!("\n{LINE}\n\n");
        assert_eq!(parse(&input).unwrap().len(), 1);
    }

    /// The `zj-picker` lesson: a schema change must be loud. Silently reading eight of nine
    /// columns would put the wrong text on every row.
    #[test]
    fn a_column_change_is_an_error_not_a_shrug() {
        let eight = LINE.rsplit_once('\t').unwrap().0;
        assert_eq!(parse(eight), Err(Error::Columns { got: 8, line: 1 }));

        let ten = format!("{LINE}\textra");
        assert_eq!(parse(&ten), Err(Error::Columns { got: 10, line: 1 }));
    }

    /// A dash in `session`/`pane` is the producer saying "not inside zellij", not a parse
    /// failure. The row still belongs in the list; it just cannot be jumped to.
    #[test]
    fn an_agent_outside_zellij_still_parses() {
        let line = LINE.replacen("\tbipa\t0\t", "\t-\t-\t", 1);
        let rows = parse(&line).unwrap();
        assert_eq!(rows[0].session, "-");
        assert_eq!(rows[0].pane, "-");
    }

    #[test]
    fn an_unreadable_number_costs_the_number_not_the_row() {
        let line = LINE.replacen("\t14493\t", "\tnonsense\t", 1);
        let rows = parse(&line).unwrap();
        assert_eq!(rows[0].transition_age_s, 0);
        assert_eq!(rows[0].name, "projeto-ponte-55");
    }
}
