use std::process::Command;

use serde::{Deserialize, Deserializer};

const PRODUCER: &str = "claude-ps";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Zellij {
    pub session: String,
    pub pane: String,
}

impl Zellij {
    pub fn address(&self) -> Option<&Self> {
        let usable = |s: &str| !s.is_empty() && !s.contains(char::is_whitespace);
        (usable(&self.session) && usable(&self.pane)).then_some(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Row {
    #[serde(rename = "status", deserialize_with = "null_as_empty")]
    pub raw_status: String,
    #[serde(rename = "status_age", alias = "age")]
    pub transition_age_s: u64,
    #[serde(default)]
    pub zellij: Option<Zellij>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub name: String,
    #[serde(default)]
    pub name_source: Option<String>,
}

fn null_as_empty<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NotFound,
    Failed { code: Option<i32> },
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

    #[test]
    fn an_unknown_key_is_not_an_error() {
        let extended = OUT.replace(
            r#""status_age": 14493,"#,
            r#""status_age": 14493, "something_new": true,"#,
        );
        assert_eq!(parse(&extended).unwrap()[0].transition_age_s, 14493);
    }

    #[test]
    fn a_renamed_status_key_stops_the_parse() {
        let renamed = OUT.replace(r#""status": "idle""#, r#""state": "idle""#);
        assert!(matches!(parse(&renamed), Err(Error::Parse(_))));
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(matches!(parse("[{]"), Err(Error::Parse(_))));
    }

    #[test]
    fn a_missing_age_stops_the_parse_rather_than_reading_zero() {
        let aged_out = OUT.replace(r#""status_age": 14493,"#, "");
        assert!(matches!(parse(&aged_out), Err(Error::Parse(_))));
    }

    #[test]
    fn the_age_key_before_the_rename_still_reads() {
        let old = OUT.replace(r#""status_age": 14493"#, r#""age": 14493"#);
        assert_eq!(parse(&old).unwrap()[0].transition_age_s, 14493);
    }

    #[test]
    fn no_agents_is_an_empty_list_not_an_error() {
        assert_eq!(parse("[]\n"), Ok(Vec::new()));
    }

    #[test]
    fn an_agent_outside_zellij_still_parses() {
        let outside = OUT.replace(r#"{ "session": "bipa", "pane": "0" }"#, "null");
        assert_eq!(parse(&outside).unwrap()[0].zellij, None);
    }

    #[test]
    fn an_absent_name_source_is_not_a_failure() {
        let nulled = OUT.replace(r#""name_source": "derived""#, r#""name_source": null"#);
        assert_eq!(parse(&nulled).unwrap()[0].name_source, None);

        let dropped = OUT.replace(r#""name_source": "derived","#, "");
        assert_eq!(parse(&dropped).unwrap()[0].name_source, None);
    }

    #[test]
    fn a_null_string_costs_the_field_not_the_row() {
        let nulled = OUT.replace(r#""status": "idle""#, r#""status": null"#);
        let rows = parse(&nulled).unwrap();
        assert_eq!(rows[0].raw_status, "");
        assert_eq!(rows[0].name, "projeto-ponte-55");
    }
}
