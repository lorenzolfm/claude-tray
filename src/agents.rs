//! The one question this applet asks the outside world: what runs now?
//!
//! This module runs `claude-ps`. It does not read `~/.claude/sessions`. That program already
//! does the pid and `procStart` liveness check that prevents a recycled pid from showing a dead
//! agent as live, and it already joins each agent to its zellij session and pane. A second
//! implementation here could disagree with the first, and `luneta` is already the second
//! consumer of the first.
//!
//! The data stays raw. `claude-ps` passes `status` through unchanged, and its README tells
//! consumers not to compare it against a fixed set. [`crate::state`] interprets it. This module
//! only says which keys the applet reads out of the JSON array.

use std::process::Command;

use serde::{Deserialize, Deserializer};

/// The program comes from `PATH`, not from a pinned store path, so `claude-ps` can be upgraded
/// without a rebuild of the applet.
const PRODUCER: &str = "claude-ps";

/// Where an agent runs, when it runs in zellij at all.
///
/// The producer sends one object, not two fields, and the applet needs it that way. An attach
/// to a session and a focus of a pane are one operation, so a session without a pane is not an
/// address that [`crate::jump`] can use. There is no state where one value is known and the
/// other is not.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Zellij {
    /// The zellij session name, which is what `zellij attach` takes.
    pub session: String,
    /// `$ZELLIJ_PANE_ID`, which is what `zellij action focus-pane-id` takes.
    pub pane: String,
}

impl Zellij {
    /// This pair as an address that [`crate::jump`] can act on, or `None` when it is not one.
    ///
    /// A session name that holds whitespace is not an address. `jump::pair_sockets` splits the
    /// rows of `ss -x -p` on spaces to learn which client is attached to which server socket,
    /// and its own doc concedes that assumption: it holds for each path that zellij builds, and
    /// not for each name that a person types. A session that breaks the split is joined to no
    /// client, `jump::focus` thus finds no terminal showing it, and the last fallthrough there
    /// attaches instead — which opens a *second* terminal for a session that is already on
    /// screen. The click looks like it worked. An empty string is not an address for the plainer
    /// reason that neither `zellij attach` nor `zellij action focus-pane-id` takes one.
    ///
    /// The producer sends the two values as one object, so they are judged as one: half an
    /// address is no more usable than none, and [`crate::state::Target`] holds a pair.
    ///
    /// Rejected here rather than in [`Deserialize`], because the row is still worth showing. A
    /// `snapshot` reads the raw session for the title and the address only through this method,
    /// so an unaddressable session gives a grey row and keeps its name, instead of vanishing
    /// from the menu with the poll.
    pub fn address(&self) -> Option<&Self> {
        let usable = |s: &str| !s.is_empty() && !s.contains(char::is_whitespace);
        (usable(&self.session) && usable(&self.pane)).then_some(self)
    }
}

/// One agent from `claude-ps`, not yet interpreted.
///
/// Only the keys that the applet reads are kept, and the other keys are ignored. This is the
/// reason to read the fields by name instead of by position: an unknown key now costs nothing,
/// but a tenth column was an [`Error::Parse`] and an empty tray.
///
/// `name` and `session` are different values. `name` is Claude Code's label. A derived label is
/// the basename of the cwd plus a two-character suffix, so `…/infra.git/master` becomes
/// `master-3c`. `zellij.session` is the zellij session that holds the agent, `infra`. Only
/// `zellij.session` is an address, and the label can have no relation to it.
///
/// `name_source` tells you who chose the label. A name that a person chose is the only string
/// in the row that tells you the purpose of the agent. The applet thus shows the chosen name
/// where there is one, and the session where there is not. See `state::chosen_name`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Row {
    /// The value from the producer, unchanged. `busy | idle | waiting | shell`, or a value that
    /// a later Claude Code version adds.
    ///
    /// Strict for the same reason as [`Row::transition_age_s`], and it had the same bug: with a
    /// `#[serde(default)]` an absent `status` key read as `""`, which parses as `Other`, and the
    /// badge counts `Waiting` alone. A rename of this key would thus empty the badge, blank the
    /// status column and leave every row a quiet `Other`, with no error anywhere. This field decides the badge, the sort, the glyph and the SNI status, so it
    /// gets at least the strictness that `status_age` already has.
    ///
    /// A `null` status stays cheap, because the producer documents `null` as a value. Only the
    /// absent key costs the poll.
    #[serde(rename = "status", deserialize_with = "null_as_empty")]
    pub raw_status: String,
    /// Seconds in the current status.
    ///
    /// This is not the time since the start of the session, and it does not increase during a
    /// busy turn. `statusUpdatedAt` records the entry into a state. On a busy row this value is
    /// the duration of the turn, which is the only indication that a session is stuck.
    ///
    /// This is the one strict field here, and it has no `#[serde(default)]` on purpose. It had
    /// a default, and the producer then changed the key from `age` to `status_age`. Each row
    /// then deserialised to `0`, and each menu line showed `<1m` for each agent. A silent zero
    /// is worse than a stopped parse, because it is a confident wrong answer in the column that
    /// shows whether an agent is stuck. An absent key now costs the poll and puts the reason in
    /// the menu, which is what the tolerance on the other fields is for.
    ///
    /// The alias keeps a `claude-ps` from before that change operational, because the two
    /// programs have no common version.
    #[serde(rename = "status_age", alias = "age")]
    pub transition_age_s: u64,
    /// `None` when the agent does not run in zellij. This is a usual state, not a failure. Such
    /// a row still counts and still shows, but it has no jump target.
    #[serde(default)]
    pub zellij: Option<Zellij>,
    /// Claude Code's label for the session, which a person or the program can choose.
    ///
    /// Show it only if a person chose it. See [`Row::name_source`] and `state::name_rows`. A
    /// derived name is the basename of the cwd plus a suffix, and the row already has an
    /// address of its own.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub name: String,
    /// Who chose [`Row::name`]: `user`, `peer`, `derived`, `collision`, `auto`, `hook`, or a
    /// value that a later Claude Code adds. `None` is the state before the key existed.
    ///
    /// This field is optional, unlike `status_age`, because the producer documents `null` as a
    /// value rather than as an absent key.
    #[serde(default)]
    pub name_source: Option<String>,
}

/// A `null` string from the producer becomes an empty string here, not an `Option`.
///
/// The producer sends `null` only where Claude Code's own file had no such field, which is a
/// change of schema rather than a usual state. The applet has no better answer than "unknown".
/// An empty status classifies as `Other`, which the badge never counts, so the applet shows the
/// agent instead of a badge for it.
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
        "status_age": 14493,
        "zellij": { "session": "bipa", "pane": "0" },
        "name": "projeto-ponte-55",
        "name_source": "derived",
        "pid": 3134390,
        "session_id": "some-uuid",
        "session_started_at": 1787965062,
        "cwd": "/home/lorenzo/p",
        "permission_mode": null
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
                name_source: Some("derived".into()),
            }]
        );
    }

    /// Under positional columns a tenth column was a hard error and an empty tray. A key that
    /// this build does not know is now no event at all.
    #[test]
    fn an_unknown_key_is_not_an_error() {
        let extended = OUT.replace(
            r#""status_age": 14493,"#,
            r#""status_age": 14493, "something_new": true,"#,
        );
        assert_eq!(parse(&extended).unwrap()[0].transition_age_s, 14493);
    }

    /// The other half of that trade: an absent key that this build depends on is still an
    /// error, because it changes what the applet shows. A renamed `status` used to read as `""`,
    /// which classifies as `Other`, which the badge never counts, so a rename showed every live
    /// agent with a blank status and a badge of zero.
    #[test]
    fn a_renamed_status_key_stops_the_parse() {
        let renamed = OUT.replace(r#""status": "idle""#, r#""state": "idle""#);
        assert!(matches!(parse(&renamed), Err(Error::Parse(_))));
    }

    /// Output that is not JSON at all is the same event as a missing key: the applet says so.
    #[test]
    fn malformed_json_is_an_error() {
        assert!(matches!(parse("[{]"), Err(Error::Parse(_))));
    }

    /// The one field with no tolerance, and the reason the other fields can have some. The
    /// change from `age` to `status_age` under a defaulted field made each row's age a
    /// confident `0`. A stopped parse reports the problem instead.
    #[test]
    fn a_missing_age_stops_the_parse_rather_than_reading_zero() {
        let aged_out = OUT.replace(r#""status_age": 14493,"#, "");
        assert!(matches!(parse(&aged_out), Err(Error::Parse(_))));
    }

    /// A `claude-ps` from before that change still works, because the two programs have no
    /// common version.
    #[test]
    fn the_age_key_before_the_rename_still_reads() {
        let old = OUT.replace(r#""status_age": 14493"#, r#""age": 14493"#);
        assert_eq!(parse(&old).unwrap()[0].transition_age_s, 14493);
    }

    /// No agents is `[]`. It must give an empty list, not a failure, or the applet shows
    /// "the producer is broken" when nothing runs.
    #[test]
    fn no_agents_is_an_empty_list_not_an_error() {
        assert_eq!(parse("[]\n"), Ok(Vec::new()));
    }

    /// A `null` join means "not in zellij", not a parse failure. The row stays in the list, but
    /// it has no jump target.
    #[test]
    fn an_agent_outside_zellij_still_parses() {
        let outside = OUT.replace(r#"{ "session": "bipa", "pane": "0" }"#, "null");
        assert_eq!(parse(&outside).unwrap()[0].zellij, None);
    }

    /// The producer documents `null` for this key. It is the state from before Claude Code
    /// recorded who named a session.
    #[test]
    fn an_absent_name_source_is_not_a_failure() {
        let nulled = OUT.replace(r#""name_source": "derived""#, r#""name_source": null"#);
        assert_eq!(parse(&nulled).unwrap()[0].name_source, None);

        let dropped = OUT.replace(r#""name_source": "derived","#, "");
        assert_eq!(parse(&dropped).unwrap()[0].name_source, None);
    }

    /// A `null` status is a change of schema. It is not a reason to remove a live agent from
    /// the list.
    #[test]
    fn a_null_string_costs_the_field_not_the_row() {
        let nulled = OUT.replace(r#""status": "idle""#, r#""status": null"#);
        let rows = parse(&nulled).unwrap();
        assert_eq!(rows[0].raw_status, "");
        assert_eq!(rows[0].name, "projeto-ponte-55");
    }
}
