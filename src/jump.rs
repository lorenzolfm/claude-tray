//! Clicking a row puts Lorenzo in front of that agent's zellij session.
//!
//! # One terminal, many sessions
//!
//! 🎯 **He runs exactly one terminal and switches zellij sessions inside it** — his words, and
//! the correction [[CSB-17]] is built on. [[CSB-16]] assumed a window *per session*, so it
//! reached for a new terminal whenever it could not find one showing the target. The right shape
//! is the other way round: **retarget the terminal he already has, and open one only when there
//! is none at all.** There is no which-window rule here because there is nothing to arbitrate.
//!
//! # Why a client's own `argv` cannot be trusted
//!
//! 🔴 **A zellij client's `argv` names the session it *originally attached to*, not the one it is
//! *currently showing*.** CSB-16 matched on argv, a value that goes stale the moment he switches
//! sessions — his normal way of working — and that one stale string produced both symptoms he
//! reported: clicking the argv's session raised a terminal displaying something else, and
//! clicking the session actually on screen matched nothing and spawned a duplicate beside it.
//!
//! **The live session comes from the socket instead.** A client is connected to exactly one
//! `zellij --server <path>/<session>` process, so pairing the two ends of that socket names the
//! session as fact. ⚠️ `/proc/net/unix` does not expose the peer inode — only `ss` does, through
//! sock_diag netlink — which is why this module shells out for it.
//!
//! ⚠️ **Title-matching stays rejected**, and CSB-17 found a fresh reason on top of the old one:
//! after a session switch the terminal's title kept naming the *previous* session until something
//! redrew it. It is a display string.
//!
//! # Why the switch is verified rather than assumed
//!
//! 🔴 **`zellij action switch-session` is delivered to the *last client that pressed a key* in
//! that session.** A CLI action carries no client id, so the server picks one — `route.rs` falls
//! back to `get_last_active_client()`, and when no client has typed there it hands the action to
//! the throwaway CLI client, which exits. The switch then silently does nothing, and there is no
//! CLI flag to name a client.
//!
//! ⚠️ **This is not a rare corner — it was the very first thing the live test hit.** He arrives at
//! a session by *switching* to it with `zj-picker`, and a client that switched in has pressed no
//! key in its new session. So on his own workflow the terminal is regularly unwakeable, and a
//! click that only tried `switch-session` would have gone on opening the duplicate window he
//! complained about.
//!
//! So the terminal is **woken** with a keystroke it will actually register, and only then asked to
//! switch. The move is still **checked** — the client's live session is read back — and only if it
//! never arrives does a terminal get opened. **Every row still lands**, which was Lorenzo's whole
//! reason for choosing raise-or-attach in CSB-16.
//!
//! # Why the wake is unconditional
//!
//! 🔴 **[[CSB-19]]: waking only when zellij refused cost a second on every click**, because
//! finding out that it refused *is* a timeout. CSB-17 tried the switch first and woke only on
//! refusal, which reads as thrifty and is not: on the common path that first switch **cannot**
//! succeed — he arrives at sessions by switching, so the client has typed nothing there — so it
//! burned the whole `SWITCH_DEADLINE` before the wake even started. That was the ~1.2 s he felt.
//!
//! ⚠️ **There is nothing to learn by asking first.** Measured on the box: a switch that is going
//! to succeed is visible in **39–65 ms** over 20 trials, and one that is going to be ignored is
//! *never* visible — still unmoved 3.2 s on. The speculative call buys no information the wake
//! does not make moot.
//!
//! Waking costs ~130 ms unconditionally, and the `Ctrl e` pair is invisible in his config. 🔴
//! **His call**, taken again with the numbers in hand: ~1.2 s → ~0.2 s, for a keystroke nobody
//! sees on the clicks that would have worked anyway.

use crate::state::Target;
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

/// The terminal opened for a session nothing is currently showing.
///
/// Hardcoded on purpose: the map scopes this slice to the nixos box alone, and this is the
/// terminal on it. A second knob here would be a config surface with one possible value.
const TERMINAL: &str = "ghostty";

/// How far up a process tree to look for the terminal window. A client sits three or four hops
/// under it; the bound is only here so a `/proc` that lies cannot spin us forever.
const MAX_ANCESTRY: usize = 32;

/// How long to wait for a switched client to turn up in its new session before giving up and
/// opening a terminal instead.
///
/// **A switch that is going to happen lands in 39–65 ms** — 20 trials against a woken lab client,
/// [[CSB-19]] — so this is some fifteen times the worst case observed.
///
/// ⚠️ **Keep the slack.** Under CSB-17 a premature expiry here was harmless, because the wake and
/// a second attempt sat behind it. Since CSB-19 the wake comes *first* and there is nothing
/// behind this: expiring means `retarget` gives up and `focus` opens the duplicate window CSB-16
/// and CSB-17 exist to prevent. It is never paid on a click that works, so the generosity is free.
const SWITCH_DEADLINE: Duration = Duration::from_millis(1000);

/// How often to re-read the client's live session while waiting. Each poll is one `ss` run.
const SWITCH_POLL: Duration = Duration::from_millis(50);

/// The keystroke that wakes a terminal's client: **`Ctrl e`**, sent twice.
///
/// 🔴 **It has to be a key zellij *consumes*, or the pane sees it.** `Ctrl e` toggles locked mode
/// in his config from every mode including locked, so a pair of them is a round trip that leaves
/// the mode exactly as it was and never reaches the program in the pane. Verified against his
/// running terminal with Claude Code in the focused pane.
///
/// Hardcoded for the same reason `TERMINAL` is — one box, one config. ⚠️ If that binding ever
/// goes away this stops being invisible: a bare `^E` would reach the pane instead.
const WAKE_KEY: &str = "e";

/// The modifier the wake key is held with, as `hl.dsp.send_shortcut` spells it.
///
/// 🔴 **It has to be the modifier's *name*, not Hyprland's numeric modmask.** `mods=4` — the
/// mask `hyprctl dispatch sendshortcut` takes — is accepted by the Lua dispatcher without a
/// word of complaint and then silently dropped: the window receives `0x65`, a bare `e`, exactly
/// as it does for `mods=0`. `mods="CTRL"` delivers `0x05`. Measured against a ghostty in raw
/// mode, one dispatch per encoding.
///
/// ⚠️ That is what made every click type `ee` into the focused pane. A bare `e` is not bound in
/// zellij, so nothing consumed it and both wake keys reached the program in the pane — the one
/// failure mode `WAKE_KEY`'s note says to watch for, arriving through the modifier rather than
/// through a lost binding.
const WAKE_MODS: &str = "CTRL";

/// Long enough for zellij to have taken the first key before the second arrives.
const WAKE_GAP: Duration = Duration::from_millis(60);

/// A terminal window with a zellij client in it, and the session that client is **showing**.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Terminal {
    /// The `zellij` client process inside the window.
    client: u32,
    /// The session it is displaying *now*, resolved from its socket peer — never from its argv.
    session: String,
    /// The pid the compositor knows this window by, found by walking the client's ancestry.
    window: u32,
}

/// Fire the jump and return what to say if it failed.
///
/// Deliberately not fatal. A tray applet that dies because a session went away between the poll
/// and the click is a worse tool than one that shrugs and repaints.
pub fn focus(target: &Target) -> Result<(), String> {
    let terminals = terminals()?;

    // Already showing it: land on the agent's pane, then bring the window up.
    if let Some(t) = terminals.iter().find(|t| t.session == target.session) {
        focus_pane(target)?;
        if raise(t.window)? {
            return Ok(());
        }
    }

    // Showing something else: retarget it in place. 🔴 `--pane-id` makes this one call — the
    // session change and the pane landing together — so there is no frame in which he could see
    // the wrong pane.
    if let Some(t) = terminals.iter().find(|t| t.session != target.session)
        && retarget(t, target)?
        && raise(t.window)?
    {
        return Ok(());
    }

    // No terminal at all, or one zellij declined to move.
    attach(&target.session)
}

/// Every terminal window on this box that has a zellij client in it.
///
/// The join is three facts, none of them a guess: `/proc` says which processes are clients, the
/// socket says which session each one is showing, and the compositor says which pids own windows.
/// A client failing any of the three — a server, our own `action` spawn, an SSH client with no
/// window here — is simply absent from the list, which is the correct answer rather than a
/// failure.
fn terminals() -> Result<Vec<Terminal>, String> {
    let windows = window_pids()?;
    let live = live_sessions()?;
    Ok(client_pids()
        .into_iter()
        .filter_map(|client| {
            Some(Terminal {
                session: live.get(&client)?.clone(),
                window: window_of(&ancestry(client), &windows)?,
                client,
            })
        })
        .collect())
}

/// Step one, when a terminal is already showing the session: move that session's focus to the
/// agent's pane.
///
/// Cheap and almost always redundant — the agent's pane is already the focused one — but not
/// always: a session he left on a different pane does need this, and it has to happen *before*
/// the window comes up so he never sees the wrong pane flash.
fn focus_pane(target: &Target) -> Result<(), String> {
    let out = zellij()
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

/// Move a terminal to another session, waking its client first so that zellij will listen.
///
/// 🔴 **The wake is unconditional** — see the module note. Asking first only bought a timeout.
///
/// `Ok(false)` means even the woken client did not move, or the window went away underneath us.
/// Either way the caller's answer is the same: open a terminal.
fn retarget(from: &Terminal, target: &Target) -> Result<bool, String> {
    // ⚠️ Raise before waking. The wake is a real key event, and the surest way for it to land in
    // this terminal is for this terminal to be the focused window — which it is about to become
    // anyway. `Ok(false)` here is the window having closed between the scan and now.
    if !raise(from.window)? {
        return Ok(false);
    }
    wake(from.window)?;
    switch(from, target)
}

/// Make this terminal's client the one zellij will listen to, by giving it a keystroke.
///
/// 🔴 **There is no polite way to do this.** `last_active_client` moves only for a real `Key`
/// message from a real client, so nothing the CLI can send will do it — see the module note. A
/// synthetic key through the compositor is the only lever, which is why `WAKE_KEY` has to be one
/// zellij swallows whole.
fn wake(window: u32) -> Result<(), String> {
    for _ in 0..2 {
        let out = Command::new("hyprctl")
            .arg("repl")
            .arg(wake_lua(window))
            .output()
            .map_err(|e| format!("hyprctl: {e}"))?;
        if String::from_utf8_lossy(&out.stdout).trim() != "sent" {
            return Err(format!("wake {window} failed"));
        }
        std::thread::sleep(WAKE_GAP);
    }
    Ok(())
}

/// ⚠️ `send_shortcut` takes the window it is aimed at, so this does not depend on the compositor's
/// idea of focus — but see `retarget`, which raises first anyway.
fn wake_lua(window: u32) -> String {
    format!(
        "for _,w in ipairs(hl.get_windows()) do if w.pid=={window} then \
         hl.dispatch(hl.dsp.send_shortcut{{mods=\"{WAKE_MODS}\",key=\"{WAKE_KEY}\",window=w}}) \
         return \"sent\" end end return \"none\""
    )
}

/// Ask zellij to retarget a terminal, landing on the agent's pane in the same call.
///
/// `Ok(false)` means zellij took the request and moved nothing — see the module note on
/// `last_active_client`. That is not an error and must not be logged as one; it is a fact about
/// which client last pressed a key, and the caller's answer to it is to open a terminal.
fn switch(from: &Terminal, target: &Target) -> Result<bool, String> {
    let out = zellij()
        .args(["--session", &from.session, "action", "switch-session"])
        .arg("--pane-id")
        .arg(format!("terminal_{}", target.pane))
        .arg(&target.session)
        .output()
        .map_err(|e| format!("zellij: {e}"))?;

    if !out.status.success() {
        let noise = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "switch {} -> {} failed: {}",
            from.session,
            target.session,
            first_line(&noise).unwrap_or("zellij said nothing")
        ));
    }
    Ok(arrived(from.client, &target.session))
}

/// Whether the client actually turned up in `session`, read back from the socket rather than
/// taken on trust. ⚠️ The exit code says only that the request was accepted.
fn arrived(client: u32, session: &str) -> bool {
    let deadline = Instant::now() + SWITCH_DEADLINE;
    loop {
        if live_sessions()
            .ok()
            .and_then(|live| live.get(&client).cloned())
            .is_some_and(|s| s == session)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(SWITCH_POLL);
    }
}

/// Bring a window to the front. `Ok(false)` means the compositor no longer knows that pid — a
/// race against a terminal closing, not a failure.
fn raise(window: u32) -> Result<bool, String> {
    let out = Command::new("hyprctl")
        .arg("repl")
        .arg(raise_lua(window))
        .output()
        .map_err(|e| format!("hyprctl: {e}"))?;

    match String::from_utf8_lossy(&out.stdout).trim() {
        "raised" => Ok(true),
        "none" => Ok(false),
        // ⚠️ Anything else is a broken compositor call, not an absent window, and the difference
        // matters: treating it as absent would open a duplicate terminal for a session that is
        // sitting there on screen.
        other => Err(format!(
            "raise {window} failed: {}",
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
/// Only a pid is interpolated, so there is nothing here to quote.
fn raise_lua(window: u32) -> String {
    format!(
        "for _,w in ipairs(hl.get_windows()) do \
         if w.pid=={window} then hl.dispatch(hl.dsp.focus{{window=w}}) return \"raised\" end \
         end return \"none\""
    )
}

/// Every pid the compositor owns a window for. Asked for as a flat list rather than parsed out of
/// `hyprctl clients -j`, so this module still needs no JSON.
fn window_pids() -> Result<Vec<u32>, String> {
    let out = Command::new("hyprctl")
        .arg("repl")
        .arg(
            "local t={} for _,w in ipairs(hl.get_windows()) do t[#t+1]=w.pid end \
             return table.concat(t,\",\")",
        )
        .output()
        .map_err(|e| format!("hyprctl: {e}"))?;

    parse_window_pids(String::from_utf8_lossy(&out.stdout).trim())
        .ok_or_else(|| "hyprctl: could not read the window list".to_string())
}

/// ⚠️ An empty answer is a desktop with no windows, which is legal. Anything non-numeric is the
/// compositor complaining, and must not be read as *no windows* — that would open a terminal for
/// a session already on screen.
fn parse_window_pids(out: &str) -> Option<Vec<u32>> {
    if out.is_empty() {
        return Some(Vec::new());
    }
    out.split(',').map(|p| p.trim().parse().ok()).collect()
}

/// The pid of the first ancestor the compositor owns a window for. The client sits a few hops
/// under the terminal emulator — client, shell, terminal — and only the compositor knows which of
/// those pids is the window.
fn window_of(chain: &[u32], windows: &[u32]) -> Option<u32> {
    chain.iter().find(|pid| windows.contains(pid)).copied()
}

/// A pid and its parents, up to the session leader.
fn ancestry(pid: u32) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut pid = pid;
    for _ in 0..MAX_ANCESTRY {
        if chain.contains(&pid) {
            break;
        }
        chain.push(pid);
        match ppid(pid) {
            Some(parent) if parent > 1 => pid = parent,
            _ => break,
        }
    }
    chain
}

/// Which session each zellij client is **currently** showing, by pairing the two ends of its
/// socket: the server end carries the session's socket path, the peer end is held by the client.
///
/// ⚠️ Only `ss` can do this — it reads peers over sock_diag netlink, and `/proc/net/unix` does not
/// carry them at all. `ss` is on the systemd unit's forced PATH.
fn live_sessions() -> Result<HashMap<u32, String>, String> {
    let out = Command::new("ss")
        .args(["-x", "-p"])
        .output()
        .map_err(|e| format!("ss: {e}"))?;
    Ok(pair_sockets(
        &String::from_utf8_lossy(&out.stdout),
        &server_sockets(),
    ))
}

/// Join `ss -x -p` rows against the known server socket paths.
///
/// A row is `netid state recv-q send-q <addr> <inode> <peer-addr> <peer-inode> users:(…)`. Rows
/// whose local address is a server's socket path name a session; the pid holding that row's
/// *peer* inode is the client attached to it.
///
/// 🔴 **The session comes from the socket path, not from the client's argv** — the whole point of
/// [[CSB-17]]. ⚠️ Splitting on whitespace assumes the socket path has none, which is true of
/// every path zellij builds.
fn pair_sockets(ss: &str, servers: &HashMap<String, String>) -> HashMap<u32, String> {
    let mut owner: HashMap<u64, u32> = HashMap::new();
    let mut served: Vec<(u64, String)> = Vec::new();

    for line in ss.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (Some(inode), Some(peer)) = (
            f.get(5).and_then(|i| i.parse::<u64>().ok()),
            f.get(7).and_then(|i| i.parse::<u64>().ok()),
        ) else {
            continue;
        };
        if let Some(pid) = pid_in(&f[8..].join(" ")) {
            owner.insert(inode, pid);
        }
        if let Some(session) = servers.get(f[4]) {
            served.push((peer, session.clone()));
        }
    }

    served
        .into_iter()
        .filter_map(|(peer, session)| Some((*owner.get(&peer)?, session)))
        .collect()
}

/// The first `pid=N` in an `ss` process column — `users:(("zellij",pid=6435,fd=79),…)`. A socket
/// held by several processes is one that was forked across; any of them names the same client.
fn pid_in(users: &str) -> Option<u32> {
    let rest = users.split_once("pid=")?.1;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Every live session's socket path, mapped to its session name, read off the servers themselves.
///
/// Taken from `--server <path>` rather than assembled from a socket directory, so nothing here
/// has to know whether zellij put its sockets under `$XDG_RUNTIME_DIR` or `/tmp`.
fn server_sockets() -> HashMap<String, String> {
    proc_argvs()
        .filter_map(|argv| {
            let path = server_socket(&argv)?;
            let session = path.rsplit('/').next()?.to_string();
            Some((path.to_string(), session))
        })
        .collect()
}

/// The socket path a `zellij --server <path>` process is serving, if that is what this argv is.
fn server_socket(argv: &[String]) -> Option<&str> {
    if !is_zellij(argv) {
        return None;
    }
    let at = argv.iter().position(|a| a == "--server")?;
    argv.get(at + 1).map(String::as_str)
}

/// Every zellij *client* process on the box.
///
/// 🔴 **Deliberately says nothing about which session.** That was CSB-16's mistake: it filtered on
/// the session name appearing in the argv, which names where the client *started*. Which session
/// a client is showing is the socket's business, not argv's.
///
/// ⚠️ Two invocations must still be excluded or they match themselves: the long-lived `--server`,
/// and the momentary `zellij --session x action …` that this very module spawns.
fn client_pids() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .map(|raw| is_client(&argv(&raw)))
                .unwrap_or(false)
        })
        .collect()
}

/// Every process's argv, for the scans that have to look at all of them.
fn proc_argvs() -> impl Iterator<Item = Vec<String>> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            Some(argv(&std::fs::read(format!("/proc/{pid}/cmdline")).ok()?))
        })
}

/// Split a `/proc/<pid>/cmdline` into arguments. NUL-separated, usually NUL-terminated.
fn argv(raw: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(raw)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_zellij(argv: &[String]) -> bool {
    argv.first()
        .is_some_and(|exe| exe.rsplit('/').next() == Some("zellij"))
}

/// Whether this argv is an attached zellij client, as opposed to a server or a CLI action.
fn is_client(argv: &[String]) -> bool {
    is_zellij(argv) && !argv.iter().any(|a| a == "--server" || a == "action")
}

/// The parent of a pid, from `/proc/<pid>/stat`.
fn ppid(pid: u32) -> Option<u32> {
    ppid_from_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// ⚠️ Field two of `stat` is the executable name **in parentheses and unescaped** — it can
/// contain spaces and parentheses of its own, so the fields after it are only safe to split from
/// the *last* `)` in the line.
fn ppid_from_stat(stat: &str) -> Option<u32> {
    stat[stat.rfind(')')? + 1..]
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Last resort: nothing on this box is showing the session and no terminal would take it, so open
/// one that does.
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
    let mut child = strip_zellij(Command::new(TERMINAL))
        .args(["-e", "zellij", "attach"])
        .arg(session)
        .spawn()
        .map_err(|e| format!("attach {session} failed: {TERMINAL}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// A `zellij` invocation that is honest about not being inside a session.
///
/// ⚠️ **Not merely tidy — `zellij action` *hangs* when `ZELLIJ_SESSION_NAME` names a session that
/// no longer exists.** A tray applet started from inside a pane would inherit exactly that once
/// the pane's session ended, and every click would then block forever on a thread nobody joins.
/// Stripping the variables removes the code path rather than racing it.
fn zellij() -> Command {
    strip_zellij(Command::new("zellij"))
}

fn strip_zellij(mut cmd: Command) -> Command {
    cmd.env_remove("ZELLIJ")
        .env_remove("ZELLIJ_SESSION_NAME")
        .env_remove("ZELLIJ_PANE_ID");
    cmd
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

    /// 🔴 The case every click in the journal actually hit. zellij calls it an error and exits 2;
    /// it means the pane was already where it should be.
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
            assert!(is_client(&a), "{a:?}");
        }
    }

    /// ⚠️ The server carries a session name too, and reaches no window. Matching it would make
    /// every detached session look attached — the one mistake that breaks the whole ticket.
    #[test]
    fn server_is_not_a_client() {
        let a = args(&[
            "/nix/store/x/bin/zellij",
            "--server",
            "/run/user/1000/infra",
        ]);
        assert!(!is_client(&a));
    }

    /// This module's own steps, caught mid-spawn, must not look like a client.
    #[test]
    fn our_own_action_is_not_a_client() {
        let a = args(&[
            "zellij",
            "--session",
            "infra",
            "action",
            "switch-session",
            "dotfiles",
        ]);
        assert!(!is_client(&a));
    }

    #[test]
    fn other_processes_are_not_clients() {
        assert!(!is_client(&args(&["fish"])));
        assert!(!is_client(&[]));
    }

    #[test]
    fn server_socket_is_the_argument_after_the_flag() {
        let a = args(&["zellij", "--server", "/run/user/1000/zellij/c1/infra"]);
        assert_eq!(server_socket(&a), Some("/run/user/1000/zellij/c1/infra"));
        assert_eq!(server_socket(&args(&["zellij", "a", "infra"])), None);
        assert_eq!(server_socket(&args(&["sshd", "--server", "/x"])), None);
    }

    fn servers(paths: &[(&str, &str)]) -> HashMap<String, String> {
        paths
            .iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect()
    }

    /// 🔴 The whole of [[CSB-17]] in one assertion: the client's argv says `dotfiles`, and the
    /// socket says it is showing `nixos`. Transcribed from a real `ss -x -p` on the box.
    #[test]
    fn the_socket_outranks_the_argv() {
        let ss = "\
u_str ESTAB 0 0 /run/user/1000/zellij/c1/nixos 324481 * 364748 users:((\"zellij\",pid=13253,fd=7),(\"zellij\",pid=13253,fd=6))
u_str ESTAB 0 0 * 364748 * 324481 users:((\"zellij\",pid=6435,fd=79),(\"zellij\",pid=6435,fd=78))";
        let live = pair_sockets(ss, &servers(&[("/run/user/1000/zellij/c1/nixos", "nixos")]));
        assert_eq!(live.get(&6435).map(String::as_str), Some("nixos"));
    }

    /// A detached session has a server and no client, and must contribute nobody.
    #[test]
    fn a_server_with_no_peer_names_no_client() {
        let ss = "\
u_str LISTEN 0 128 /run/user/1000/zellij/c1/infra 700 * 0 users:((\"zellij\",pid=8691,fd=5))";
        let live = pair_sockets(ss, &servers(&[("/run/user/1000/zellij/c1/infra", "infra")]));
        assert!(live.is_empty(), "{live:?}");
    }

    /// ⚠️ Sockets that are nothing to do with zellij share the table and must be ignored, as must
    /// the header row.
    #[test]
    fn unrelated_sockets_are_ignored() {
        let ss = "\
Netid State Recv-Q Send-Q Local Address:Port Peer Address:Port Process
u_str ESTAB 0 0 /run/user/1000/wayland-1 900 * 901 users:((\"ghostty\",pid=5091,fd=3))";
        let live = pair_sockets(ss, &servers(&[("/run/user/1000/zellij/c1/nixos", "nixos")]));
        assert!(live.is_empty(), "{live:?}");
    }

    #[test]
    fn pid_is_read_out_of_the_users_column() {
        assert_eq!(pid_in("users:((\"zellij\",pid=6435,fd=79))"), Some(6435));
        assert_eq!(
            pid_in("users:((\"a\",pid=1,fd=2),(\"a\",pid=9,fd=3))"),
            Some(1)
        );
        assert_eq!(pid_in(""), None);
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

    /// Client, shell, terminal: the terminal is the first one the compositor has heard of.
    #[test]
    fn the_window_is_the_first_ancestor_with_one() {
        assert_eq!(
            window_of(&[6435, 5353, 5091, 1], &[5091, 62859]),
            Some(5091)
        );
        assert_eq!(window_of(&[6435, 5353], &[5091]), None);
    }

    /// ⚠️ No windows and a broken compositor call must not look alike — one means *open a
    /// terminal*, the other means *something is wrong*.
    #[test]
    fn window_pids_tell_empty_from_broken() {
        assert_eq!(parse_window_pids("5091,62859"), Some(vec![5091, 62859]));
        assert_eq!(parse_window_pids(""), Some(vec![]));
        assert_eq!(parse_window_pids("Lua error: nope"), None);
    }

    /// ⚠️ The wake is only invisible while it is a key zellij consumes — `Ctrl e`, its
    /// locked-mode toggle. A wrong modifier here reaches whatever is running in the pane.
    #[test]
    fn wake_lua_sends_ctrl_e_to_one_window() {
        let lua = wake_lua(5091);
        assert!(lua.contains("w.pid==5091"), "{lua}");
        // 🔴 The modifier is a name, not the numeric modmask — `mods=4` is dropped in silence
        // and the pane receives a bare `e`. See `WAKE_MODS`.
        assert!(lua.contains("mods=\"CTRL\",key=\"e\""), "{lua}");
        assert!(lua.contains("hl.dsp.send_shortcut"), "{lua}");
    }

    #[test]
    fn raise_lua_targets_one_window() {
        let lua = raise_lua(5091);
        assert!(lua.contains("w.pid==5091"), "{lua}");
        assert!(lua.contains("hl.dsp.focus{window=w}"), "{lua}");
        assert!(lua.ends_with("return \"none\""), "{lua}");
    }
}
