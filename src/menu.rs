//! What a menu row looks like.
//!
//! The word in a row, its rank and its age come from [`claude_nav::state`] and
//! are not decided here. This file holds only what belongs to a GTK menu drawn
//! through `libdbusmenu-gtk3`: a column width, and a spinner heavy enough to be
//! visible in it.

use claude_nav::state::{Entry, humanise};

const NAME_WIDTH: usize = 28;

/// Seven dots to a frame where luneta spends three.
///
/// This is the one place where the two surfaces differ on purpose, and it is
/// ink and not meaning. The GTK theme draws a menu row in its own foreground
/// colour, so three dots become grey specks that may or may not turn. Colour is
/// the smaller change and is not available: Waybar draws this menu through
/// `libdbusmenu-gtk3`, which calls `g_markup_escape_text` on each label, so
/// markup for one glyph shows as angle brackets.
///
/// Which status turns is still [`claude_nav`]'s decision. Only the weight of
/// the frames belongs here.
const SPINNER: [&str; 8] = ["⣾ ", "⣽ ", "⣻ ", "⢿ ", "⡿ ", "⣟ ", "⣯ ", "⣷ "];

pub fn label(entry: &Entry, frame: u64, since: u64) -> String {
    format!(
        "{}  {:<width$}  {} {}",
        glyph(entry, frame),
        truncate(&entry.title, NAME_WIDTH),
        entry.status.as_str(),
        humanise(entry.age(since)),
        width = NAME_WIDTH,
    )
}

fn glyph(entry: &Entry, frame: u64) -> &'static str {
    entry
        .status
        .fixed_glyph()
        .unwrap_or_else(|| SPINNER[(frame as usize) % SPINNER.len()])
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
    use claude_nav::state::Status;

    fn entry(status: &str, age_s: u64) -> Entry {
        Entry {
            status: Status::parse(status.into()),
            title: "infra".into(),
            age_s,
            target: None,
        }
    }

    #[test]
    fn only_busy_turns_and_it_turns_seven_dots() {
        let busy = entry("busy", 5);
        assert_ne!(glyph(&busy, 0), glyph(&busy, 1));
        assert_eq!(glyph(&busy, 0), glyph(&busy, SPINNER.len() as u64));
        for frame in SPINNER {
            assert_eq!(frame.trim().chars().count(), 1, "{frame:?}");
        }

        let idle = entry("idle", 5);
        assert_eq!(glyph(&idle, 0), "☕");
        assert_eq!(glyph(&idle, 0), glyph(&idle, 7));
    }

    #[test]
    fn a_row_prints_the_status_it_was_given() {
        let row = label(&entry("compacting", 120), 0, 0);
        assert!(row.contains("compacting"), "{row}");
        assert!(row.starts_with("🛸"), "{row}");
    }

    #[test]
    fn a_rows_age_counts_on_while_the_menu_is_open() {
        assert!(label(&entry("idle", 60), 0, 0).ends_with("idle 1m"));
        assert!(label(&entry("idle", 60), 0, 120).ends_with("idle 3m"));
    }

    #[test]
    fn truncation_keeps_the_tail() {
        assert_eq!(truncate("short", NAME_WIDTH), "short");
        let long = truncate("projeto-ponte-longissimo-nome-55:12", NAME_WIDTH);
        assert_eq!(long.chars().count(), NAME_WIDTH);
        assert!(long.ends_with(":12"), "{long}");
        assert!(long.contains('\u{2026}'), "{long}");
    }
}
