//! A click on a row moves you to that agent's zellij session.
//!
//! # One terminal, many sessions
//!
//! There is one terminal, and the person changes zellij sessions inside it. The rule is thus to
//! change the session in the terminal that already exists, and to open a terminal only if there
//! is none. An earlier design assumed one window for each session and opened a new terminal when
//! no window showed the target. There is no rule here to select between windows, because there
//! is nothing to select.
//!
//! # Why the `argv` of a client is not sufficient
//!
//! The `argv` of a zellij client names the session that it attached to first, not the session
//! that it shows now. A match on argv thus uses a value that becomes wrong as soon as the person
//! changes session, which is the usual way to work. That one value caused two failures: a click
//! on the session in the argv raised a terminal that showed another session, and a click on the
//! session on screen matched nothing and opened a second terminal.
//!
//! The live session comes from the socket instead. A client connects to exactly one
//! `zellij --server <path>/<session>` process, so a pair of the two ends of that socket gives the
//! session. `/proc/net/unix` does not give the peer inode, and only `ss` does, over sock_diag
//! netlink. This module therefore runs `ss`.
//!
//! A match on the window title is also not sufficient. After a change of session, the title of
//! the terminal continues to name the previous session until something draws it again. It is a
//! display string.
//!
//! # Why this code verifies the change of session
//!
//! zellij sends `zellij action switch-session` to the last client that pressed a key in that
//! session. A CLI action carries no client id, so the server selects one: `route.rs` calls
//! `get_last_active_client()`. If no client has typed there, the server sends the action to the
//! temporary CLI client, which then exits. The change of session thus does nothing, and there is
//! no CLI option to name a client.
//!
//! This is not a rare condition. A person arrives at a session by a change of session with
//! `luneta`, and a client that arrived in that way has pressed no key in its new session. The
//! terminal is thus regularly unable to receive the action, and a click that only tried
//! `switch-session` would open a second window.
//!
//! This code therefore sends a keystroke that the client registers, and only then asks for the
//! change of session. It then verifies the change: it reads the live session of the client
//! again. It opens a terminal only if the client never arrives. Each row thus reaches its agent.
//!
//! # Why the keystroke is unconditional
//!
//! A keystroke only after a refusal costs one second at each click, because the refusal is a
//! timeout. An earlier design tried the change first and sent the keystroke only after a
//! refusal. On the usual path that first change cannot succeed, because the person arrives at a
//! session by a change of session and the client has typed nothing there. That design thus used
//! the full `SWITCH_DEADLINE` before the keystroke, which was about 1.2 s.
//!
//! There is also nothing to learn from an attempt first. Measured on this machine: a change that
//! succeeds is visible in 39–65 ms over 20 trials, and a change that is ignored is never
//! visible, even 3.2 s later.
//!
//! The keystroke costs about 130 ms in each case, and the `Ctrl e` pair is invisible in this
//! configuration. The result is about 0.2 s instead of about 1.2 s, for a keystroke that nobody
//! sees on the clicks that would have succeeded.
//!
//! # Why this code finds the compositor handle
//!
//! `hyprctl` needs `HYPRLAND_INSTANCE_SIGNATURE`, and this applet starts before that value
//! exists. systemd starts the user session at boot, and Hyprland is a process inside it. The
//! environment of the unit is thus fixed before the compositor exists and never receives the
//! signature. An `import-environment` would have to run after the applet and not before it.
//! Without a solution, each `hyprctl` call failed, and each click stopped in [`terminals`] with
//! no window raised.
//!
//! That failure was also silent. `hyprctl` prints `HYPRLAND_INSTANCE_SIGNATURE not set!` on
//! stdout and exits 0: the exit code reports success, and the answer is text. No code in this
//! module thus reads an exit code from it. Each call examines the output instead, and text is
//! not a window list. See [`hyprctl`], which finds the signature in the runtime directory when
//! the environment does not supply it.

use crate::state::Target;
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

/// The terminal to open for a session that no window shows.
///
/// This value is fixed on purpose. This program runs on one machine, and this is the terminal on
/// it. An option here would be a configuration item with one possible value.
const TERMINAL: &str = "ghostty";

/// How far to go up a process tree to find the terminal window. A client is three or four steps
/// below it. This limit prevents an infinite loop if `/proc` gives wrong data.
const MAX_ANCESTRY: usize = 32;

/// How long to wait for a client to appear in its new session before this code opens a terminal
/// instead.
///
/// A change of session that succeeds completes in 39–65 ms, over 20 trials against a client that
/// had received a keystroke. This limit is thus about fifteen times the worst measured value.
///
/// Keep the limit high. The keystroke comes first and there is no second attempt after this
/// wait, so an early expiry makes `retarget` stop and `focus` open a second window. A click that
/// succeeds never waits for the full limit, so the high value costs nothing.
const SWITCH_DEADLINE: Duration = Duration::from_millis(1000);

/// How often to read the live session of the client during the wait. Each poll runs `ss` one
/// time.
const SWITCH_POLL: Duration = Duration::from_millis(50);

/// The keystroke that makes a client of a terminal active: `Ctrl e`, sent two times.
///
/// zellij must consume the key, or the pane receives it. In this configuration, `Ctrl e` toggles
/// locked mode from each mode, including locked mode. Two of them thus return the mode to its
/// initial value and never reach the program in the pane. This was verified against a running
/// terminal with Claude Code in the focused pane.
///
/// This value is fixed for the same reason as `TERMINAL`: one machine, one configuration. If
/// that key binding is removed, the keystroke becomes visible, because a plain `^E` then reaches
/// the pane.
const WAKE_KEY: &str = "e";

/// The modifier for the key above, in the form that `hl.dsp.send_shortcut` needs.
///
/// This must be the name of the modifier and not Hyprland's numeric mask. The Lua dispatcher
/// accepts `mods=4`, which is the mask that `hyprctl dispatch sendshortcut` takes, and then
/// removes it: the window receives `0x65`, a plain `e`, as it does for `mods=0`. `mods="CTRL"`
/// delivers `0x05`. This was measured against a ghostty in raw mode, with one dispatch for each
/// form.
///
/// A wrong value here made each click type `ee` into the focused pane. zellij has no binding for
/// a plain `e`, so nothing consumed it and both keys reached the program in the pane.
const WAKE_MODS: &str = "CTRL";

/// Sufficient time for zellij to accept the first key before the second key arrives.
const WAKE_GAP: Duration = Duration::from_millis(60);

/// A terminal window with a zellij client in it, and the session that the client shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Terminal {
    /// The `zellij` client process inside the window.
    client: u32,
    /// The session that it shows now, from its socket peer and never from its argv.
    session: String,
    /// The pid that the compositor uses for this window, found in the parents of the client.
    window: u32,
}

/// Do the jump, and return a message if it fails.
///
/// A failure here is not fatal on purpose. A tray applet that stops because a session ended
/// between the poll and the click is worse than one that continues and repaints.
pub fn focus(target: &Target) -> Result<(), String> {
    let terminals = terminals()?;

    // Already showing it: land on the agent's pane, then bring the window up.
    if let Some(t) = terminals.iter().find(|t| t.session == target.session) {
        focus_pane(target)?;
        if raise(t.window)? {
            return Ok(());
        }
    }

    // The terminal shows another session, so change the session in it. `--pane-id` makes this
    // one call, with the change of session and the change of pane together, so no frame shows
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

/// Each terminal window on this machine that holds a zellij client.
///
/// The join uses three sources of data. `/proc` gives the processes that are clients, the socket
/// gives the session that each client shows, and the compositor gives the pids that own windows.
/// A client that fails one of the three tests is absent from the list, which is the correct
/// result. Examples are a server, an `action` process from this module, and an SSH client with
/// no window here.
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

/// Step one, when a terminal already shows the session: move the focus of that session to the
/// pane of the agent.
///
/// This is cheap and usually unnecessary, because the pane of the agent already has the focus.
/// But a session that the person left on a different pane needs it. It must occur before the
/// window comes to the front, so that no frame shows the wrong pane.
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

/// Read the answer of zellij to `focus-pane-id`. This is a separate function because the three
/// conditions are not obvious, and each one was measured:
///
/// - a focus that occurs exits 0, with no output on the two streams;
/// - `already focused` exits 2, which is an error code for a condition that needs no action, so
///   this code must read it as a success or each click reports a failure;
/// - a session that ended exits 0 and prints the list of live sessions on stdout, which is why
///   success is silence and not the exit code.
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

/// Move a terminal to another session. The client receives a keystroke first, so that zellij
/// accepts the action.
///
/// The keystroke is unconditional. See the note on this module: an attempt first only added a
/// timeout.
///
/// `Ok(false)` means that the client did not move, or that the window ended. The answer of the
/// caller is the same in the two conditions: open a terminal.
fn retarget(from: &Terminal, target: &Target) -> Result<bool, String> {
    // Raise the window before the keystroke. The keystroke is a real key event, and it reaches
    // this terminal most reliably if this terminal has the focus, which it receives in any case.
    // `Ok(false)` here means that the window closed after the scan.
    if !raise(from.window)? {
        return Ok(false);
    }
    wake(from.window)?;
    switch(from, target)
}

/// Make the client of this terminal the client that zellij accepts actions from, with a
/// keystroke.
///
/// There is no other method. `last_active_client` changes only for a real `Key` message from a
/// real client, so no CLI command can change it. See the note on this module. A synthetic key
/// through the compositor is the only method, which is why zellij must consume `WAKE_KEY`.
fn wake(window: u32) -> Result<(), String> {
    for _ in 0..2 {
        let out = hyprctl()
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

/// `send_shortcut` takes the target window, so this does not depend on the focus in the
/// compositor. But `retarget` raises the window first in any case.
fn wake_lua(window: u32) -> String {
    format!(
        "for _,w in ipairs(hl.get_windows()) do if w.pid=={window} then \
         hl.dispatch(hl.dsp.send_shortcut{{mods=\"{WAKE_MODS}\",key=\"{WAKE_KEY}\",window=w}}) \
         return \"sent\" end end return \"none\""
    )
}

/// Ask zellij to change the session in a terminal, and to focus the pane of the agent in the
/// same call.
///
/// `Ok(false)` means that zellij accepted the request and moved nothing. See the note on this
/// module about `last_active_client`. That is not an error and must not go to the log as one. It
/// reports which client last pressed a key, and the caller then opens a terminal.
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

/// Did the client arrive in `session`? This reads the socket again instead of trust in the exit
/// code, which reports only that zellij accepted the request.
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

/// Bring a window to the front. `Ok(false)` means that the compositor does not know that pid,
/// which is a race with a terminal that closes and not a failure.
fn raise(window: u32) -> Result<bool, String> {
    let out = hyprctl()
        .arg("repl")
        .arg(raise_lua(window))
        .output()
        .map_err(|e| format!("hyprctl: {e}"))?;

    match String::from_utf8_lossy(&out.stdout).trim() {
        "raised" => Ok(true),
        "none" => Ok(false),
        // Another value means a failed compositor call and not an absent window. The difference
        // is important: an absent window opens a second terminal for a session that is already
        // on screen.
        other => Err(format!(
            "raise {window} failed: {}",
            first_line(other).unwrap_or("hyprctl said nothing")
        )),
    }
}

/// The compositor part of the raise, as a Lua expression.
///
/// Hyprland 0.56 moved `hyprctl dispatch` to a Lua API. The documented
/// `hyprctl dispatch focuswindow address:0x…` is a syntax error here, and there is no
/// `focuswindow` in `hl.dsp.window.*`. The dispatcher is `hl.dsp.focus`, and it takes a table.
/// This code uses `repl` and not `eval`, because `eval` prints `ok` and discards the result. The
/// result is necessary, because it reports that there was no window.
///
/// The code inserts a pid only, so there is nothing to quote.
fn raise_lua(window: u32) -> String {
    format!(
        "for _,w in ipairs(hl.get_windows()) do \
         if w.pid=={window} then hl.dispatch(hl.dsp.focus{{window=w}}) return \"raised\" end \
         end return \"none\""
    )
}

/// Each pid that owns a window in the compositor. The request gives a flat list, instead of a
/// parse of `hyprctl clients -j`, so this module needs no JSON.
fn window_pids() -> Result<Vec<u32>, String> {
    let out = hyprctl()
        .arg("repl")
        .arg(
            "local t={} for _,w in ipairs(hl.get_windows()) do t[#t+1]=w.pid end \
             return table.concat(t,\",\")",
        )
        .output()
        .map_err(|e| format!("hyprctl: {e}"))?;

    // The message goes into the error and is not discarded. `hyprctl` writes a refusal on
    // stdout and exits 0, so this string is the only record of the reason. The text "could not
    // read the window list" alone sent one real failure to the journal every five seconds for
    // one day and never named `HYPRLAND_INSTANCE_SIGNATURE`.
    let said = String::from_utf8_lossy(&out.stdout);
    let said = said.trim();
    parse_window_pids(said).ok_or_else(|| {
        format!(
            "hyprctl: could not read the window list: {}",
            first_line(said).unwrap_or("it said nothing")
        )
    })
}

/// An empty answer means a desktop with no windows, which is a valid state. A value that is not
/// a number is a message from the compositor, and this code must not read it as "no windows",
/// because that opens a terminal for a session that is already on screen.
fn parse_window_pids(out: &str) -> Option<Vec<u32>> {
    if out.is_empty() {
        return Some(Vec::new());
    }
    out.split(',').map(|p| p.trim().parse().ok()).collect()
}

/// The pid of the first parent that owns a window in the compositor. The client is some steps
/// below the terminal emulator, in the order client, shell, terminal. Only the compositor knows
/// which of those pids is the window.
fn window_of(chain: &[u32], windows: &[u32]) -> Option<u32> {
    chain.iter().find(|pid| windows.contains(pid)).copied()
}

/// A pid and its parents, to the session leader.
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

/// Which session each zellij client shows now. This pairs the two ends of its socket: the
/// server end holds the socket path of the session, and the client holds the peer end.
///
/// Only `ss` can do this, because it reads peers over sock_diag netlink and `/proc/net/unix`
/// does not hold them. `ss` is on the PATH that the systemd unit sets.
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

/// Join the rows of `ss -x -p` with the known socket paths of the servers.
///
/// A row is `netid state recv-q send-q <addr> <inode> <peer-addr> <peer-inode> users:(…)`. A row
/// whose local address is the socket path of a server names a session. The pid that holds the
/// peer inode of that row is the client that attached to it.
///
/// The session comes from the socket path and not from the argv of the client. The split on
/// spaces assumes that the socket path has none, which is true of each path that zellij
/// builds.
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

/// The first `pid=N` in a process column of `ss`: `users:(("zellij",pid=6435,fd=79),…)`. If
/// several processes hold one socket, a fork occurred, and each of them names the same client.
fn pid_in(users: &str) -> Option<u32> {
    let rest = users.split_once("pid=")?.1;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// The socket path of each live session, with its session name, from the servers themselves.
///
/// This comes from `--server <path>` and not from a socket directory, so no code here must know
/// whether zellij puts its sockets in `$XDG_RUNTIME_DIR` or in `/tmp`.
fn server_sockets() -> HashMap<String, String> {
    proc_argvs()
        .filter_map(|argv| {
            let path = server_socket(&argv)?;
            let session = path.rsplit('/').next()?.to_string();
            Some((path.to_string(), session))
        })
        .collect()
}

/// The socket path that a `zellij --server <path>` process serves, if this argv is one.
fn server_socket(argv: &[String]) -> Option<&str> {
    if !is_zellij(argv) {
        return None;
    }
    let at = argv.iter().position(|a| a == "--server")?;
    argv.get(at + 1).map(String::as_str)
}

/// Each zellij client process on this machine.
///
/// This gives no session on purpose. An earlier design filtered on the session name in the argv,
/// which names where the client started. The socket gives the session that a client shows, and
/// the argv does not.
///
/// Two other processes must stay out of this list: the `--server` process, and the short
/// `zellij --session x action …` process that this module starts.
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

/// The argv of each process, for the scans that must examine all of them.
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

/// Split a `/proc/<pid>/cmdline` into arguments. NUL separates them, and one usually ends the
/// data.
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

/// Is this argv an attached zellij client, and not a server or a CLI action?
fn is_client(argv: &[String]) -> bool {
    is_zellij(argv) && !argv.iter().any(|a| a == "--server" || a == "action")
}

/// The parent of a pid, from `/proc/<pid>/stat`.
fn ppid(pid: u32) -> Option<u32> {
    ppid_from_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// Field two of `stat` is the name of the executable, in parentheses and without escapes. It
/// can contain spaces and parentheses, so a split of the fields after it must start at the last
/// `)` in the line.
fn ppid_from_stat(stat: &str) -> Option<u32> {
    stat[stat.rfind(')')? + 1..]
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// The last method: no window on this machine shows the session, and no terminal accepted it, so
/// open a terminal that does.
///
/// The zellij variables must go. If the applet starts inside a zellij pane, it inherits
/// `ZELLIJ_SESSION_NAME`, and `zellij attach` then panics with "You are trying to attach to the
/// current session. This is not supported" for each session. The new terminal is inside no
/// session, so an environment without those variables is correct as well as operational. The
/// environment of the systemd unit has no such variables, which would hide this failure until a
/// person started the applet by hand.
///
/// A separate thread waits for the child process, so that it cannot become a zombie. The
/// terminal has its own lifetime, and the applet neither owns it nor continues after it.
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

/// The environment variable that `hyprctl` uses to find its compositor.
const HYPR_SIGNATURE: &str = "HYPRLAND_INSTANCE_SIGNATURE";

/// A `hyprctl` command that knows its compositor.
///
/// This code finds the signature because the process cannot inherit it. See the note on this
/// module. systemd starts the applet at boot, Hyprland starts inside that session afterwards,
/// and the environment of the unit is a copy from before the compositor existed. Without this,
/// `terminals` fails at its first call and each click does nothing.
///
/// An environment that has the value wins, because that is the compositor that the person looks
/// at. It is also the only correct answer if a second compositor runs.
fn hyprctl() -> Command {
    let mut cmd = Command::new("hyprctl");
    if std::env::var_os(HYPR_SIGNATURE).is_none()
        && let Some(signature) = hypr_signature()
    {
        cmd.env(HYPR_SIGNATURE, signature);
    }
    cmd
}

/// The signature of the running compositor, from `$XDG_RUNTIME_DIR/hypr`, in the layout that
/// `hyprctl` uses: one directory for each instance, with the signature as its name and the
/// socket inside it.
///
/// A directory is not an instance, but a socket is. Hyprland leaves the directory and its log
/// after it exits, so a machine with a restart of the compositor has several directories and
/// only one of them accepts a connection. A test for `.socket.sock` removes the old
/// directories, and the newest of the remainder is the live one.
fn hypr_signature() -> Option<std::ffi::OsString> {
    live_instance(&runtime_dir()?.join("hypr"))
}

/// The newest instance directory in `hypr` that still holds a socket.
///
/// The directory is an argument, so a test can verify the rule above without a restart of a
/// compositor.
fn live_instance(hypr: &std::path::Path) -> Option<std::ffi::OsString> {
    let mut instances: Vec<(std::time::SystemTime, std::ffi::OsString)> = std::fs::read_dir(hypr)
        .ok()?
        .flatten()
        .filter(|e| e.path().join(".socket.sock").exists())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.file_name())))
        .collect();
    instances.sort_by_key(|(modified, _)| *modified);
    instances.pop().map(|(_, name)| name)
}

/// `$XDG_RUNTIME_DIR`, or the path that systemd uses for it.
///
/// The second path exists for the same reason as this function: a unit that starts before the
/// graphical session can also have no value for this variable, and the user manager has already
/// mounted the directory at `/run/user/<uid>`. The uid comes from `/proc/self`, so this needs no
/// libc for one number.
fn runtime_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Some(std::path::PathBuf::from(dir));
    }
    use std::os::unix::fs::MetadataExt as _;
    let uid = std::fs::metadata("/proc/self").ok()?.uid();
    Some(std::path::PathBuf::from(format!("/run/user/{uid}")))
}

/// A `zellij` command that reports that it is not inside a session.
///
/// This is necessary: `zellij action` stops and waits when `ZELLIJ_SESSION_NAME` names a session
/// that no longer exists. A tray applet that starts inside a pane inherits that value after the
/// session of the pane ends, and each click then blocks a thread that nobody joins. The removal
/// of the variables removes that condition.
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

    /// The condition that each click in the journal caused. zellij reports an error and exits
    /// 2, but the pane was already in the correct place.
    #[test]
    fn already_focused_is_success() {
        assert!(pane_outcome(false, "", "Pane Terminal(0) is already focused").is_ok());
    }

    #[test]
    fn missing_pane_is_failure() {
        let e = pane_outcome(false, "", "Pane with id Terminal(99) not found").unwrap_err();
        assert!(e.contains("not found"), "{e}");
    }

    /// A session that ended exits 0 and prints the live sessions on stdout.
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

    /// The server also carries a session name, and it reaches no window. A match on it would
    /// make each detached session look attached.
    #[test]
    fn server_is_not_a_client() {
        let a = args(&[
            "/nix/store/x/bin/zellij",
            "--server",
            "/run/user/1000/infra",
        ]);
        assert!(!is_client(&a));
    }

    /// A process that this module starts must not look like a client.
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

    /// The argv of the client says `dotfiles`, and the socket says that it shows `nixos`. This
    /// data comes from a real `ss -x -p` on the machine.
    #[test]
    fn the_socket_outranks_the_argv() {
        let ss = "\
u_str ESTAB 0 0 /run/user/1000/zellij/c1/nixos 324481 * 364748 users:((\"zellij\",pid=13253,fd=7),(\"zellij\",pid=13253,fd=6))
u_str ESTAB 0 0 * 364748 * 324481 users:((\"zellij\",pid=6435,fd=79),(\"zellij\",pid=6435,fd=78))";
        let live = pair_sockets(ss, &servers(&[("/run/user/1000/zellij/c1/nixos", "nixos")]));
        assert_eq!(live.get(&6435).map(String::as_str), Some("nixos"));
    }

    /// A detached session has a server and no client, so it adds no row.
    #[test]
    fn a_server_with_no_peer_names_no_client() {
        let ss = "\
u_str LISTEN 0 128 /run/user/1000/zellij/c1/infra 700 * 0 users:((\"zellij\",pid=8691,fd=5))";
        let live = pair_sockets(ss, &servers(&[("/run/user/1000/zellij/c1/infra", "infra")]));
        assert!(live.is_empty(), "{live:?}");
    }

    /// The table also holds sockets from other programs. This code must ignore them and the
    /// header row.
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

    /// The comm field has no escapes, so a process with the name `(evil) 1 2` would break a
    /// simple split.
    #[test]
    fn ppid_survives_a_hostile_comm() {
        assert_eq!(
            ppid_from_stat("6435 (zellij) S 5353 6435 5353").unwrap(),
            5353
        );
        assert_eq!(ppid_from_stat("42 (x) 1 2) S 99 42 1").unwrap(), 99);
        assert_eq!(ppid_from_stat("no parens here"), None);
    }

    /// In the order client, shell, terminal, the terminal is the first pid that the compositor
    /// knows.
    #[test]
    fn the_window_is_the_first_ancestor_with_one() {
        assert_eq!(
            window_of(&[6435, 5353, 5091, 1], &[5091, 62859]),
            Some(5091)
        );
        assert_eq!(window_of(&[6435, 5353], &[5091]), None);
    }

    /// No windows and a failed compositor call must give different results. The first means to
    /// open a terminal, and the second means a failure.
    #[test]
    fn window_pids_tell_empty_from_broken() {
        assert_eq!(parse_window_pids("5091,62859"), Some(vec![5091, 62859]));
        assert_eq!(parse_window_pids(""), Some(vec![]));
        assert_eq!(parse_window_pids("Lua error: nope"), None);
    }

    /// The failure that stopped each click: an applet that starts before the compositor has no
    /// `HYPRLAND_INSTANCE_SIGNATURE`, and `hyprctl` then writes this text on stdout and exits 0.
    /// This code must read it as a failure and never as "no windows", because that opens a
    /// second terminal for a session that is already on screen.
    #[test]
    fn hyprctl_refusing_to_answer_is_not_an_empty_desktop() {
        assert_eq!(
            parse_window_pids("HYPRLAND_INSTANCE_SIGNATURE not set! (is hyprland running?)"),
            None
        );
    }

    /// A compositor leaves its directory after it exits. Only the socket shows that an instance
    /// is live.
    #[test]
    fn the_live_instance_is_the_newest_one_with_a_socket() {
        let root = std::env::temp_dir().join(format!("claude-tray-hypr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        // Two old directories and one live instance, created oldest first, so that the
        // modification times give the order.
        for (name, socket) in [("dead_old", true), ("live", true), ("logs_only", false)] {
            std::fs::create_dir_all(root.join(name)).unwrap();
            if socket {
                std::fs::write(root.join(name).join(".socket.sock"), "").unwrap();
            }
        }
        // `logs_only` is the newest and has no socket, so `live` must win.
        let touch = |name: &str| {
            std::fs::write(root.join(name).join("hyprland.log"), "x").unwrap();
        };
        touch("dead_old");
        std::thread::sleep(std::time::Duration::from_millis(20));
        touch("live");
        std::thread::sleep(std::time::Duration::from_millis(20));
        touch("logs_only");

        assert_eq!(live_instance(&root).as_deref(), Some("live".as_ref()));

        let empty = root.join("nothing-here");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(live_instance(&empty), None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The keystroke stays invisible only while zellij consumes the key. `Ctrl e` toggles
    /// locked mode. A wrong modifier here reaches the program in the pane.
    #[test]
    fn wake_lua_sends_ctrl_e_to_one_window() {
        let lua = wake_lua(5091);
        assert!(lua.contains("w.pid==5091"), "{lua}");
        // The modifier is a name and not the numeric mask. Hyprland discards `mods=4` without
        // a message, and the pane then receives a plain `e`. See `WAKE_MODS`.
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
