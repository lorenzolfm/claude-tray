//! Clicking a row puts Lorenzo in front of that agent's zellij session.
//!
//! # Why this is three steps and not one
//!
//! [[CSB-12]] shipped only the first step — `zellij action focus-pane-id`, addressed at a session
//! by name from outside it — and ruled window-raise out because *nothing links a Hyprland window
//! to the zellij session running inside it*. [[CSB-16]] probed the shipped applet and found both
//! halves of that wrong:
//!
//! - 🔴 **The pane jump is a no-op by construction.** The agent's pane is already the focused
//!   pane in its own session, so zellij answers `Pane Terminal(0) is already focused` and there
//!   is nothing to do. Every click in the journal said this. The pane jump was never the missing
//!   piece; the window always was.
//! - 🔴 **A window *is* linked to its session, exactly.** The attached `zellij` **client** sits
//!   inside the terminal's process tree, so walking its parents reaches a pid that appears
//!   verbatim as a Hyprland window's `pid`. That is an identity, not a title match — which is
//!   what CSB-12 rejected, and rightly.
//! - 🔴 **Half his sessions have no window at all.** A detached session has a live server and no
//!   client anywhere, so no raise can help; the only thing that can put him in front of it is a
//!   terminal that attaches. For a detached session that is not guesswork either — it is the
//!   only available answer.
//!
//! So: focus the pane, raise the window that is showing the session, and if nothing is showing
//! it, open something that does. Lorenzo chose raise-or-attach over raise-only precisely so that
//! **every row lands** rather than one row in four.

use crate::state::Target;
use std::process::Command;

/// The terminal opened for a session nothing is currently showing.
///
/// Hardcoded on purpose: the map scopes this slice to the nixos box alone, and this is the
/// terminal on it. A second knob here would be a config surface with one possible value.
const TERMINAL: &str = "ghostty";

/// How far up a process tree to look for the terminal window. A client sits three or four hops
/// under it; the bound is only here so a `/proc` that lies cannot spin us forever.
const MAX_ANCESTRY: usize = 32;

/// Fire the jump and return what to say if it failed.
///
/// Deliberately not fatal. A tray applet that dies because a session went away between the poll
/// and the click is a worse tool than one that shrugs and repaints.
pub fn focus(target: &Target) -> Result<(), String> {
    focus_pane(target)?;
    if raise_window(&target.session)? {
        return Ok(());
    }
    attach(&target.session)
}

/// Step one: move the session's own focus to the agent's pane.
///
/// Cheap and almost always redundant — see the module note — but not always: a session he left
/// on a different pane does need this, and it has to happen *before* the window comes up so he
/// never sees the wrong pane flash.
fn focus_pane(target: &Target) -> Result<(), String> {
    let out = Command::new("zellij")
        .args(["--session", &target.session, "action", "focus-pane-id"])
        .arg(&target.pane)
        .output()
        .map_err(|e| format!("zellij: {e}"))?;

    let noise = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    pane_outcome(
        out.status.success(),
        &noise(&out.stdout),
        &noise(&out.stderr),
    )
    .map_err(|why| format!("focus {}:{} failed: {why}", target.session, target.pane))
}

/// Read zellij's answer to `focus-pane-id`. Split out because the three cases are not obvious
/// and each one was observed rather than assumed:
///
/// - a real focus exits **0**, silent on both streams;
/// - **`already focused` exits 2** — an error code for the case where the primitive did
///   everything it could, so it has to be read as success or every click logs a failure;
/// - a session that has gone away exits **0** and prints the live session list on stdout, which
///   is why silence rather than the exit code is what success is matched on.
fn pane_outcome(ok: bool, stdout: &str, stderr: &str) -> Result<(), String> {
    if ok && stdout.is_empty() && stderr.is_empty() {
        return Ok(());
    }
    if stderr.contains("is already focused") {
        return Ok(());
    }
    let why = [stderr, stdout]
        .into_iter()
        .filter_map(first_line)
        .next()
        .unwrap_or("no such session?");
    Err(why.to_string())
}

/// Step two: raise whichever window is already showing this session. `Ok(false)` means nothing
/// on this box is showing it — which is a fact about the desktop, not a failure.
fn raise_window(session: &str) -> Result<bool, String> {
    let pids = window_candidates(session);
    if pids.is_empty() {
        return Ok(false);
    }

    let out = Command::new("hyprctl")
        .arg("repl")
        .arg(raise_lua(&pids))
        .output()
        .map_err(|e| format!("hyprctl: {e}"))?;

    match String::from_utf8_lossy(&out.stdout).trim() {
        "raised" => Ok(true),
        "none" => Ok(false),
        // ⚠️ Anything else is a broken compositor call, not an absent window, and the difference
        // matters: treating it as absent would open a duplicate terminal for a session that is
        // sitting there on screen.
        other => Err(format!(
            "raise {session} failed: {}",
            first_line(other).unwrap_or("hyprctl said nothing")
        )),
    }
}

/// The compositor half of the raise, as a Lua expression.
///
/// ⚠️ **Hyprland 0.56 moved `hyprctl dispatch` onto a Lua API.** The documented
/// `hyprctl dispatch focuswindow address:0x…` is a *syntax error* here, and there is no
/// `focuswindow` under `hl.dsp.window.*` — the dispatcher is `hl.dsp.focus`, which takes a table.
/// `repl` is used rather than `eval` because `eval` prints `ok` and discards the result, and the
/// result is the whole point: it is how we learn there was no window.
///
/// Only pids are interpolated, so there is nothing here to quote.
fn raise_lua(pids: &[u32]) -> String {
    let want: String = pids.iter().map(|p| format!("[{p}]=true,")).collect();
    format!(
        "local want={{{want}}} \
         for _,w in ipairs(hl.get_windows()) do \
         if want[w.pid] then hl.dispatch(hl.dsp.focus{{window=w}}) return \"raised\" end \
         end return \"none\""
    )
}

/// Every pid that could be the window showing `session`: each attached client, plus its
/// ancestors up to the session leader. One of them is the terminal emulator; the compositor
/// says which, because only it knows which pids own windows.
fn window_candidates(session: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    for client in clients_of(session) {
        let mut pid = client;
        for _ in 0..MAX_ANCESTRY {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
            match ppid(pid) {
                Some(parent) if parent > 1 => pid = parent,
                _ => break,
            }
        }
    }
    pids
}

/// The attached `zellij` clients for a session, by scanning `/proc` for their argv.
///
/// ⚠️ **The client is not the server.** A detached session still has a `zellij --server` process
/// carrying `ZELLIJ_SESSION_NAME`, but it hangs off the init system and reaches no window —
/// which is exactly the case this whole function exists to report as *empty*.
fn clients_of(session: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .map(|raw| is_client_of(&argv(&raw), session))
                .unwrap_or(false)
        })
        .collect()
}

/// Split a `/proc/<pid>/cmdline` into arguments. NUL-separated, usually NUL-terminated.
fn argv(raw: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(raw)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether this argv is a zellij *client* attached to `session`.
///
/// Deliberately loose about *how* he attached — `zellij a x`, `zellij attach x` and
/// `zellij --session x` all name the session as a bare argument, so matching a whole argument
/// against a known session name is both simpler and harder to break than parsing flags.
/// ⚠️ Two invocations must be excluded or they match themselves: the long-lived `--server`, and
/// the momentary `zellij --session x action …` that this very module spawns.
fn is_client_of(argv: &[String], session: &str) -> bool {
    let Some(exe) = argv.first() else {
        return false;
    };
    if exe.rsplit('/').next() != Some("zellij") {
        return false;
    }
    if argv.iter().any(|a| a == "--server" || a == "action") {
        return false;
    }
    argv[1..].iter().any(|a| a == session)
}

/// The parent of a pid, from `/proc/<pid>/stat`.
fn ppid(pid: u32) -> Option<u32> {
    ppid_from_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// ⚠️ Field two of `stat` is the executable name **in parentheses and unescaped** — it can
/// contain spaces and parentheses of its own, so the fields after it are only safe to split
/// from the *last* `)` in the line.
fn ppid_from_stat(stat: &str) -> Option<u32> {
    stat[stat.rfind(')')? + 1..]
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Step three: nothing is showing this session, so open something that does.
///
/// 🔴 **The zellij variables have to go.** If the applet was itself started from inside a zellij
/// pane it inherits `ZELLIJ_SESSION_NAME`, and `zellij attach` then *panics* — "You are trying to
/// attach to the current session. This is not supported" — for every session, not just that one.
/// The new terminal is not inside anything, so saying so is the truthful environment as well as
/// the working one. Observed while testing [[CSB-16]]; the systemd unit's environment happens to
/// be clean, which is exactly what would have kept this hidden until he ran it by hand.
///
/// The child is waited on in a detached thread purely so it cannot linger as a zombie — the
/// terminal's lifetime is its own, and the applet neither owns nor outlives it.
fn attach(session: &str) -> Result<(), String> {
    let mut child = Command::new(TERMINAL)
        .args(["-e", "zellij", "attach"])
        .arg(session)
        .env_remove("ZELLIJ")
        .env_remove("ZELLIJ_SESSION_NAME")
        .env_remove("ZELLIJ_PANE_ID")
        .spawn()
        .map_err(|e| format!("attach {session} failed: {TERMINAL}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn first_line(s: &str) -> Option<&str> {
    s.lines().map(str::trim).find(|l| !l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn silent_success_is_success() {
        assert!(pane_outcome(true, "", "").is_ok());
    }

    /// 🔴 The case every click in the journal actually hit. zellij calls it an error and exits
    /// 2; it means the pane was already where it should be.
    #[test]
    fn already_focused_is_success() {
        assert!(pane_outcome(false, "", "Pane Terminal(0) is already focused").is_ok());
    }

    #[test]
    fn missing_pane_is_failure() {
        let e = pane_outcome(false, "", "Pane with id Terminal(99) not found").unwrap_err();
        assert!(e.contains("not found"), "{e}");
    }

    /// A session that has gone away exits 0 and prints the live sessions on stdout.
    #[test]
    fn chatty_exit_zero_is_failure() {
        assert!(pane_outcome(true, "Sessions:\ndotfiles", "").is_err());
    }

    #[test]
    fn client_matches_every_attach_spelling() {
        for a in [
            args(&["zellij", "a", "infra"]),
            args(&["zellij", "attach", "infra"]),
            args(&["/nix/store/x/bin/zellij", "--session", "infra"]),
        ] {
            assert!(is_client_of(&a, "infra"), "{a:?}");
        }
    }

    /// ⚠️ The server carries the session name too, and reaches no window. Matching it would make
    /// every detached session look attached — the one mistake that breaks the whole ticket.
    #[test]
    fn server_is_not_a_client() {
        let a = args(&[
            "/nix/store/x/bin/zellij",
            "--server",
            "/run/user/1000/infra",
        ]);
        assert!(!is_client_of(&a, "infra"));
    }

    /// This module's own step one, caught mid-spawn, must not look like a client.
    #[test]
    fn our_own_action_is_not_a_client() {
        let a = args(&[
            "zellij",
            "--session",
            "infra",
            "action",
            "focus-pane-id",
            "0",
        ]);
        assert!(!is_client_of(&a, "infra"));
    }

    #[test]
    fn other_sessions_do_not_match() {
        assert!(!is_client_of(&args(&["zellij", "a", "dotfiles"]), "infra"));
        assert!(!is_client_of(&args(&["fish"]), "infra"));
        assert!(!is_client_of(&[], "infra"));
    }

    /// The comm field is unescaped, so a process named `(evil) 1 2` would derail a naive split.
    #[test]
    fn ppid_survives_a_hostile_comm() {
        assert_eq!(
            ppid_from_stat("6435 (zellij) S 5353 6435 5353").unwrap(),
            5353
        );
        assert_eq!(ppid_from_stat("42 (x) 1 2) S 99 42 1").unwrap(), 99);
        assert_eq!(ppid_from_stat("no parens here"), None);
    }

    #[test]
    fn raise_lua_builds_a_pid_set() {
        let lua = raise_lua(&[5091, 5353]);
        assert!(lua.contains("[5091]=true,[5353]=true"), "{lua}");
        assert!(lua.contains("hl.dsp.focus{window=w}"), "{lua}");
        assert!(lua.ends_with("return \"none\""), "{lua}");
    }
}
