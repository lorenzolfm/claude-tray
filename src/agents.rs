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
//! [`crate::state`]'s job, not this module's. Everything here stays at the level of "a JSON
//! array arrived, here are the keys this applet reads".

use std::process::Command;

use serde::{Deserialize, Deserializer};

/// The program is looked up on `PATH` rather than pinned to a store path, so `claude-ps`
/// can be upgraded underneath the applet without rebuilding it.
const PRODUCER: &str = "claude-ps";

/// Where an agent is sitting, when it is sitting in zellij at all.
///
/// 🔴 One object, not two fields, and the producer emits it that way for the reason this
/// applet needs: attaching to a session and focusing a pane is a **single act**, so a session
/// without a pane is an address [`crate::jump`] cannot use. There is no state where one is
/// known and the other is not.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Zellij {
    /// The zellij session name, and what `zellij attach` takes.
    pub session: String,
    /// `$ZELLIJ_PANE_ID`, which is exactly what `zellij action focus-pane-id` takes.
    pub pane: String,
}

/// One agent out of `claude-ps`, still uninterpreted.
///
/// Only the keys this applet reads are kept; the rest of the object is ignored. ⚠️ That is
/// the point of moving off positional columns — an unknown key costs nothing now, where a
/// tenth column used to be a hard [`Error::Parse`] and a blind tray.
///
/// 🔴 **`name` and `session` are two different things, and calling both of them "the session
/// name" is what [[CSB-15]] cost.** `name` is Claude Code's own label — the cwd basename plus a
/// two-character suffix, so `…/infra.git/master` becomes `master-3c`. `zellij.session` is the
/// zellij session that agent is sitting in, `infra`. Only the second is an address, and the
/// first can be unrelated to it. [[CSB-2]] justified dropping `cwd` on the grounds that "the
/// session name already *is* its basename", which is true of `name` and false of
/// `zellij.session`; rows are named by the latter now, and `name` survives only as the fallback
/// for an agent outside zellij.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Row {
    /// Verbatim from the producer. `busy | idle | waiting | shell`, or anything a future
    /// Claude Code version invents.
    #[serde(rename = "status", default, deserialize_with = "null_as_empty")]
    pub raw_status: String,
    /// Seconds in the *current* status.
    ///
    /// ⚠️ Not time since the session started, and it does **not** advance during a busy turn —
    /// `statusUpdatedAt` marks entry into a state. On a working row this reads as turn
    /// duration, which is the only surface a wedged session shows up on.
    #[serde(rename = "age", default)]
    pub transition_age_s: u64,
    /// `None` when the agent is not inside zellij — an ordinary state, not a failure. Such a
    /// row still counts and still renders; it simply has nowhere to jump to.
    #[serde(default)]
    pub zellij: Option<Zellij>,
    /// Claude Code's own label for the session, derived by it from the cwd basename.
    ///
    /// ⚠️ **Not what a row is named by** — see the note above, and `state::name_rows`.
    /// It renders only when `zellij` is `None`, where it is the sole identifier left.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub name: String,
    /// Wall-clock unix seconds at session start.
    ///
    /// ⚠️ This is `startedAt`, not `procStart` — the latter is jiffies-since-boot and is
    /// meaningless in arithmetic against a timestamp. Newborn suppression needs this one.
    #[serde(default)]
    pub started_at: u64,
}

/// A `null` string from the producer becomes empty here rather than `Option`.
///
/// The producer emits `null` only where Claude Code's own file lacked the field entirely, which
/// is a schema move rather than an ordinary state. The applet has no better answer than
/// "unknown", and an empty status classifies as `Working` — never actionable, so it renders the
/// agent rather than inventing a badge for it.
fn null_as_empty<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `claude-ps` is not on `PATH`, or could not be executed.
    NotFound,
    /// It ran and exited non-zero.
    Failed { code: Option<i32> },
    /// It ran, exited zero, and emitted something this build cannot deserialise.
    Parse(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "{PRODUCER} not found on PATH"),
            Error::Failed { code: Some(c) } => write!(f, "{PRODUCER} exited {c}"),
            Error::Failed { code: None } => write!(f, "{PRODUCER} was killed"),
            Error::Parse(why) => write!(f, "{PRODUCER}: {why}"),
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

/// Deserialise the array. Pure, so the schema contract can be tested without a process.
pub fn parse(stdout: &str) -> Result<Vec<Row>, Error> {
    serde_json::from_str(stdout).map_err(|e| Error::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = r#"[
      {
        "status": "idle",
        "age": 14493,
        "zellij": { "session": "bipa", "pane": "0" },
        "name": "projeto-ponte-55",
        "pid": 3134390,
        "session_id": "some-uuid",
        "started_at": 1787965062,
        "cwd": "/home/lorenzo/p"
      }
    ]"#;

    #[test]
    fn reads_the_keys_it_needs() {
        assert_eq!(
            parse(OUT).unwrap(),
            vec![Row {
                raw_status: "idle".into(),
                transition_age_s: 14493,
                zellij: Some(Zellij {
                    session: "bipa".into(),
                    pane: "0".into()
                }),
                name: "projeto-ponte-55".into(),
                started_at: 1787965062,
            }]
        );
    }

    /// 🔴 The inversion this change bought. Under positional columns a tenth column was a hard
    /// error and a blind tray; the producer gaining `started_at` is exactly what that cost.
    /// A key this build has never heard of is now a non-event.
    #[test]
    fn an_unknown_key_is_not_an_error() {
        let extended = OUT.replace(
            r#""age": 14493,"#,
            r#""age": 14493, "something_new": true,"#,
        );
        assert_eq!(parse(&extended).unwrap()[0].transition_age_s, 14493);
    }

    /// ⚠️ The other half of that trade: a key this build *depends* on going missing is still
    /// loud, because it changes what the applet would render.
    #[test]
    fn a_renamed_key_is_still_an_error() {
        let renamed = OUT.replace(r#""status": "idle""#, r#""state": "idle""#);
        assert!(matches!(parse(&renamed), Ok(rows) if rows[0].raw_status.is_empty()));

        assert!(matches!(parse("[{]"), Err(Error::Parse(_))));
    }

    /// 🔴 No agents is `[]`, and must be an empty list rather than a failure — otherwise
    /// "nothing is running" renders as "the producer is broken".
    #[test]
    fn no_agents_is_an_empty_list_not_an_error() {
        assert_eq!(parse("[]\n"), Ok(Vec::new()));
    }

    /// A `null` join is the producer saying "not inside zellij", not a parse failure. The row
    /// still belongs in the list; it just cannot be jumped to.
    #[test]
    fn an_agent_outside_zellij_still_parses() {
        let outside = OUT.replace(r#"{ "session": "bipa", "pane": "0" }"#, "null");
        assert_eq!(parse(&outside).unwrap()[0].zellij, None);
    }

    /// A `null` status is a schema move, not a reason to drop a live agent off the list.
    #[test]
    fn a_null_string_costs_the_field_not_the_row() {
        let nulled = OUT.replace(r#""status": "idle""#, r#""status": null"#);
        let rows = parse(&nulled).unwrap();
        assert_eq!(rows[0].raw_status, "");
        assert_eq!(rows[0].name, "projeto-ponte-55");
    }
}
