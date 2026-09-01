use crate::state::Target;
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

const TERMINAL: &str = "ghostty";

const MAX_ANCESTRY: usize = 32;

const SWITCH_DEADLINE: Duration = Duration::from_millis(1000);

const SWITCH_POLL: Duration = Duration::from_millis(50);

const WAKE_KEY: &str = "e";

const WAKE_MODS: &str = "CTRL";

const WAKE_GAP: Duration = Duration::from_millis(60);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Terminal {
    client: u32,
    session: String,
    window: u32,
}

pub fn focus(target: &Target) -> Result<(), String> {
    let terminals = terminals()?;

    if let Some(t) = terminals.iter().find(|t| t.session == target.session) {
        focus_pane(target)?;
        if raise(t.window)? {
            return Ok(());
        }
    }

    if let Some(t) = terminals.iter().find(|t| t.session != target.session)
        && retarget(t, target)?
        && raise(t.window)?
    {
        return Ok(());
    }

    attach(&target.session)
}

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

fn retarget(from: &Terminal, target: &Target) -> Result<bool, String> {
    if !raise(from.window)? {
        return Ok(false);
    }
    wake(from.window)?;
    switch(from, target)
}

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

fn wake_lua(window: u32) -> String {
    format!(
        "for _,w in ipairs(hl.get_windows()) do if w.pid=={window} then \
         hl.dispatch(hl.dsp.send_shortcut{{mods=\"{WAKE_MODS}\",key=\"{WAKE_KEY}\",window=w}}) \
         return \"sent\" end end return \"none\""
    )
}

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

fn raise(window: u32) -> Result<bool, String> {
    let out = hyprctl()
        .arg("repl")
        .arg(raise_lua(window))
        .output()
        .map_err(|e| format!("hyprctl: {e}"))?;

    match String::from_utf8_lossy(&out.stdout).trim() {
        "raised" => Ok(true),
        "none" => Ok(false),
        other => Err(format!(
            "raise {window} failed: {}",
            first_line(other).unwrap_or("hyprctl said nothing")
        )),
    }
}

fn raise_lua(window: u32) -> String {
    format!(
        "for _,w in ipairs(hl.get_windows()) do \
         if w.pid=={window} then hl.dispatch(hl.dsp.focus{{window=w}}) return \"raised\" end \
         end return \"none\""
    )
}

fn window_pids() -> Result<Vec<u32>, String> {
    let out = hyprctl()
        .arg("repl")
        .arg(
            "local t={} for _,w in ipairs(hl.get_windows()) do t[#t+1]=w.pid end \
             return table.concat(t,\",\")",
        )
        .output()
        .map_err(|e| format!("hyprctl: {e}"))?;

    let said = String::from_utf8_lossy(&out.stdout);
    let said = said.trim();
    parse_window_pids(said).ok_or_else(|| {
        format!(
            "hyprctl: could not read the window list: {}",
            first_line(said).unwrap_or("it said nothing")
        )
    })
}

fn parse_window_pids(out: &str) -> Option<Vec<u32>> {
    if out.is_empty() {
        return Some(Vec::new());
    }
    out.split(',').map(|p| p.trim().parse().ok()).collect()
}

fn window_of(chain: &[u32], windows: &[u32]) -> Option<u32> {
    chain.iter().find(|pid| windows.contains(pid)).copied()
}

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

fn pid_in(users: &str) -> Option<u32> {
    let rest = users.split_once("pid=")?.1;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn server_sockets() -> HashMap<String, String> {
    proc_argvs()
        .filter_map(|argv| {
            let path = server_socket(&argv)?;
            let session = path.rsplit('/').next()?.to_string();
            Some((path.to_string(), session))
        })
        .collect()
}

fn server_socket(argv: &[String]) -> Option<&str> {
    if !is_zellij(argv) {
        return None;
    }
    let at = argv.iter().position(|a| a == "--server")?;
    argv.get(at + 1).map(String::as_str)
}

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

fn is_client(argv: &[String]) -> bool {
    is_zellij(argv) && !argv.iter().any(|a| a == "--server" || a == "action")
}

fn ppid(pid: u32) -> Option<u32> {
    ppid_from_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn ppid_from_stat(stat: &str) -> Option<u32> {
    stat[stat.rfind(')')? + 1..]
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

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

const HYPR_SIGNATURE: &str = "HYPRLAND_INSTANCE_SIGNATURE";

fn hyprctl() -> Command {
    let mut cmd = Command::new("hyprctl");
    if std::env::var_os(HYPR_SIGNATURE).is_none()
        && let Some(signature) = hypr_signature()
    {
        cmd.env(HYPR_SIGNATURE, signature);
    }
    cmd
}

fn hypr_signature() -> Option<std::ffi::OsString> {
    live_instance(&runtime_dir()?.join("hypr"))
}

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

fn runtime_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Some(std::path::PathBuf::from(dir));
    }
    use std::os::unix::fs::MetadataExt as _;
    let uid = std::fs::metadata("/proc/self").ok()?.uid();
    Some(std::path::PathBuf::from(format!("/run/user/{uid}")))
}

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

    #[test]
    fn already_focused_is_success() {
        assert!(pane_outcome(false, "", "Pane Terminal(0) is already focused").is_ok());
    }

    #[test]
    fn missing_pane_is_failure() {
        let e = pane_outcome(false, "", "Pane with id Terminal(99) not found").unwrap_err();
        assert!(e.contains("not found"), "{e}");
    }

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

    #[test]
    fn server_is_not_a_client() {
        let a = args(&[
            "/nix/store/x/bin/zellij",
            "--server",
            "/run/user/1000/infra",
        ]);
        assert!(!is_client(&a));
    }

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

    #[test]
    fn the_socket_outranks_the_argv() {
        let ss = "\
u_str ESTAB 0 0 /run/user/1000/zellij/c1/nixos 324481 * 364748 users:((\"zellij\",pid=13253,fd=7),(\"zellij\",pid=13253,fd=6))
u_str ESTAB 0 0 * 364748 * 324481 users:((\"zellij\",pid=6435,fd=79),(\"zellij\",pid=6435,fd=78))";
        let live = pair_sockets(ss, &servers(&[("/run/user/1000/zellij/c1/nixos", "nixos")]));
        assert_eq!(live.get(&6435).map(String::as_str), Some("nixos"));
    }

    #[test]
    fn a_server_with_no_peer_names_no_client() {
        let ss = "\
u_str LISTEN 0 128 /run/user/1000/zellij/c1/infra 700 * 0 users:((\"zellij\",pid=8691,fd=5))";
        let live = pair_sockets(ss, &servers(&[("/run/user/1000/zellij/c1/infra", "infra")]));
        assert!(live.is_empty(), "{live:?}");
    }

    #[test]
    fn a_session_named_with_a_space_is_never_joined_to_its_client() {
        let ss = "\
u_str ESTAB 0 0 /run/user/1000/zellij/c1/my work 324481 * 364748 users:((\"zellij\",pid=13253,fd=7))
u_str ESTAB 0 0 * 364748 * 324481 users:((\"zellij\",pid=6435,fd=79))";
        let live = pair_sockets(
            ss,
            &servers(&[("/run/user/1000/zellij/c1/my work", "my work")]),
        );
        assert!(live.is_empty(), "{live:?}");
    }

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
    fn the_window_is_the_first_ancestor_with_one() {
        assert_eq!(
            window_of(&[6435, 5353, 5091, 1], &[5091, 62859]),
            Some(5091)
        );
        assert_eq!(window_of(&[6435, 5353], &[5091]), None);
    }

    #[test]
    fn window_pids_tell_empty_from_broken() {
        assert_eq!(parse_window_pids("5091,62859"), Some(vec![5091, 62859]));
        assert_eq!(parse_window_pids(""), Some(vec![]));
        assert_eq!(parse_window_pids("Lua error: nope"), None);
    }

    #[test]
    fn hyprctl_refusing_to_answer_is_not_an_empty_desktop() {
        assert_eq!(
            parse_window_pids("HYPRLAND_INSTANCE_SIGNATURE not set! (is hyprland running?)"),
            None
        );
    }

    #[test]
    fn the_live_instance_is_the_newest_one_with_a_socket() {
        let root = std::env::temp_dir().join(format!("claude-tray-hypr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        for (name, socket) in [("dead_old", true), ("live", true), ("logs_only", false)] {
            std::fs::create_dir_all(root.join(name)).unwrap();
            if socket {
                std::fs::write(root.join(name).join(".socket.sock"), "").unwrap();
            }
        }
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

    #[test]
    fn wake_lua_sends_ctrl_e_to_one_window() {
        let lua = wake_lua(5091);
        assert!(lua.contains("w.pid==5091"), "{lua}");
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
