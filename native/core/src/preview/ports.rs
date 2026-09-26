//! Finding the project's dev servers.
//!
//! Two sources, merged by port:
//! * URLs a background process printed (`Local: http://localhost:5173/`).
//! * Listening TCP sockets (`/proc/net/tcp{,6}`) whose owning process (found
//!   through the socket inodes in `/proc/<pid>/fd`) runs with its working
//!   folder inside the project (`/proc/<pid>/cwd`).
//!
//! Only sockets reachable over loopback count: bound to a loopback address or
//! to the wildcard address. Everything takes a `/proc` root so tests can use
//! fixture trees.
use regex::Regex;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    sync::LazyLock,
};

/// A listening TCP socket from `/proc/net/tcp` or `/proc/net/tcp6`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    pub ip: IpAddr,
    pub port: u16,
    pub inode: u64,
}

/// A process of this project listening on a loopback-reachable port.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Found {
    pub port: u16,
    pub ip: IpAddr,
    pub pid: u32,
    /// `/proc/<pid>/comm` (the short process name).
    pub process: String,
    /// The command line, bounded.
    pub command: String,
    pub cwd: PathBuf,
}

const LISTEN: &str = "0A";

/// Parse `/proc/net/tcp` (`v6 == false`) or `/proc/net/tcp6` text into its
/// listening sockets. Malformed lines are skipped.
pub fn parse_net(text: &str, v6: bool) -> Vec<Listener> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 || fields[3] != LISTEN {
                return None;
            }
            let (ip, port) = parse_address(fields[1], v6)?;
            let inode = fields[9].parse().ok()?;
            (inode != 0).then_some(Listener { ip, port, inode })
        })
        .collect()
}

/// `0100007F:1F90` → 127.0.0.1:8080. The kernel prints each 32-bit word of the
/// network-order address as a native-endian number.
fn parse_address(text: &str, v6: bool) -> Option<(IpAddr, u16)> {
    let (address, port) = text.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let word = |chunk: &str| u32::from_str_radix(chunk, 16).ok().map(u32::to_ne_bytes);
    let ip = if v6 {
        if address.len() != 32 || !address.is_ascii() {
            return None;
        }
        let mut bytes = [0u8; 16];
        for i in 0..4 {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word(&address[i * 8..i * 8 + 8])?);
        }
        IpAddr::V6(Ipv6Addr::from(bytes))
    } else {
        if address.len() != 8 {
            return None;
        }
        IpAddr::V4(Ipv4Addr::from(word(address)?))
    };
    Some((ip, port))
}

/// A browser on this computer can reach the socket through loopback.
pub fn loopback_reachable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_unspecified(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

/// Every loopback-reachable listening socket in this network namespace.
pub fn listening(proc_root: &Path) -> Vec<Listener> {
    let mut all = Vec::new();
    for (file, v6) in [("net/tcp", false), ("net/tcp6", true)] {
        if let Ok(text) = fs::read_to_string(proc_root.join(file)) {
            all.extend(
                parse_net(&text, v6)
                    .into_iter()
                    .filter(|l| loopback_reachable(l.ip)),
            );
        }
    }
    all
}

/// Socket inodes a process holds open (`/proc/<pid>/fd/* -> socket:[N]`).
/// Other users' processes are unreadable and yield nothing.
fn socket_inodes(proc_root: &Path, pid: u32) -> HashSet<u64> {
    let Ok(entries) = fs::read_dir(proc_root.join(pid.to_string()).join("fd")) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let link = fs::read_link(entry.path()).ok()?;
            let link = link.to_str()?;
            link.strip_prefix("socket:[")?
                .strip_suffix(']')?
                .parse()
                .ok()
        })
        .collect()
}

fn pids(proc_root: &Path) -> Vec<u32> {
    fs::read_dir(proc_root)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn read_trimmed(path: PathBuf, limit: usize) -> String {
    let text = fs::read(path).unwrap_or_default();
    let text = String::from_utf8_lossy(&text[..text.len().min(limit)])
        .replace('\0', " ")
        .trim()
        .to_owned();
    text
}

/// Processes whose working folder is inside `project` and that listen on a
/// loopback-reachable port. `exclude_pid` (ShadowCode itself) is skipped.
pub fn project_servers(proc_root: &Path, project: &Path, exclude_pid: u32) -> Vec<Found> {
    let listeners = listening(proc_root);
    if listeners.is_empty() {
        return Vec::new();
    }
    let by_inode: HashMap<u64, &Listener> = listeners.iter().map(|l| (l.inode, l)).collect();
    let project = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let mut found = Vec::new();
    for pid in pids(proc_root) {
        if pid == exclude_pid {
            continue;
        }
        let base = proc_root.join(pid.to_string());
        // The working folder first: it is one readlink and rules out almost
        // every process before its descriptors are listed.
        let Ok(cwd) = fs::read_link(base.join("cwd")) else {
            continue;
        };
        if !cwd.starts_with(&project) {
            continue;
        }
        for inode in socket_inodes(proc_root, pid) {
            let Some(listener) = by_inode.get(&inode) else {
                continue;
            };
            found.push(Found {
                port: listener.port,
                ip: listener.ip,
                pid,
                process: read_trimmed(base.join("comm"), 64),
                command: read_trimmed(base.join("cmdline"), 400),
                cwd: cwd.clone(),
            });
        }
    }
    found.sort_by_key(|f| (f.port, f.pid));
    found.dedup_by_key(|f| f.port);
    found
}

/// Ports `pid` itself listens on: never a preview target.
pub fn own_ports(proc_root: &Path, pid: u32) -> HashSet<u16> {
    let inodes = socket_inodes(proc_root, pid);
    let mut all = Vec::new();
    for (file, v6) in [("net/tcp", false), ("net/tcp6", true)] {
        if let Ok(text) = fs::read_to_string(proc_root.join(file)) {
            all.extend(parse_net(&text, v6));
        }
    }
    all.into_iter()
        .filter(|l| inodes.contains(&l.inode))
        .map(|l| l.port)
        .collect()
}

/// A local URL printed in a process's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintedUrl {
    /// Normalised for the preview (`0.0.0.0` and `[::]` read as `localhost`).
    pub url: String,
    pub port: u16,
}

static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)").unwrap()
});
static LOCAL_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bhttp://(localhost|127\.\d{1,3}\.\d{1,3}\.\d{1,3}|\[::1?\]|0\.0\.0\.0|[a-z0-9-]+(?:\.[a-z0-9-]+)*\.localhost):(\d{1,5})(/[^\s'\x22<>`)\]]*)?",
    )
    .unwrap()
});

/// Local `http://` URLs with an explicit port in `output` (terminal colours
/// removed), first occurrence of each port, in order.
pub fn printed_urls(output: &str) -> Vec<PrintedUrl> {
    let plain = ANSI.replace_all(output, "");
    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    for capture in LOCAL_URL.captures_iter(&plain) {
        let Ok(port) = capture[2].parse::<u16>() else {
            continue;
        };
        if port == 0 || !seen.insert(port) {
            continue;
        }
        let host = capture[1].to_ascii_lowercase();
        let host = match host.as_str() {
            "0.0.0.0" | "[::]" => "localhost".to_owned(),
            _ => host,
        };
        let path = capture
            .get(3)
            .map(|m| m.as_str().trim_end_matches(['.', ',', ';', ':']))
            .unwrap_or("/");
        urls.push(PrintedUrl {
            url: format!(
                "http://{host}:{port}{}",
                if path.is_empty() { "/" } else { path }
            ),
            port,
        });
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    // Captured on x86-64 (little-endian words).
    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41001 1 0000000000000000 100 0 0 10 0
   1: 00000000:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41002 1 0000000000000000 100 0 0 10 0
   2: 0501A8C0:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41003 1 0000000000000000 100 0 0 10 0
   3: 0100007F:1435 0100007F:A1B2 01 00000000:00000000 00:00000000 00000000  1000        0 41004 1 0000000000000000 20 4 30 10 -1
   4: 0100007F:2328 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41005 1 0000000000000000 100 0 0 10 0
   5: garbage
";
    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:0FA0 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 42001 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:1F40 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 42002 1 0000000000000000 100 0 0 10 0
   2: 000080FE00000000FF005450B6AC08FE:1388 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 42003 1 0000000000000000 100 0 0 10 0
";

    #[cfg(target_endian = "little")]
    #[test]
    fn parses_proc_net_listeners() {
        let v4 = parse_net(TCP, false);
        assert_eq!(
            v4,
            vec![
                Listener {
                    ip: "127.0.0.1".parse().unwrap(),
                    port: 5173,
                    inode: 41001
                },
                Listener {
                    ip: "0.0.0.0".parse().unwrap(),
                    port: 3000,
                    inode: 41002
                },
                Listener {
                    ip: "192.168.1.5".parse().unwrap(),
                    port: 8080,
                    inode: 41003
                },
                Listener {
                    ip: "127.0.0.1".parse().unwrap(),
                    port: 9000,
                    inode: 41005
                },
            ]
        );
        let v6 = parse_net(TCP6, true);
        assert_eq!(v6[0].ip, "::1".parse::<IpAddr>().unwrap());
        assert_eq!(v6[0].port, 4000);
        assert_eq!(v6[1].ip, "::".parse::<IpAddr>().unwrap());
        assert_eq!(
            v6[2].ip,
            "fe80::5054:ff:fe08:acb6".parse::<IpAddr>().unwrap()
        );
        assert!(loopback_reachable(v4[0].ip) && loopback_reachable(v4[1].ip));
        assert!(!loopback_reachable(v4[2].ip));
        assert!(loopback_reachable(v6[0].ip) && loopback_reachable(v6[1].ip));
        assert!(!loopback_reachable(v6[2].ip));
        assert!(loopback_reachable("::ffff:127.0.0.1".parse().unwrap()));
    }

    fn process(root: &Path, pid: u32, cwd: &Path, comm: &str, cmdline: &str, inodes: &[u64]) {
        let base = root.join(pid.to_string());
        fs::create_dir_all(base.join("fd")).unwrap();
        symlink(cwd, base.join("cwd")).unwrap();
        fs::write(base.join("comm"), format!("{comm}\n")).unwrap();
        fs::write(base.join("cmdline"), cmdline.replace(' ', "\0")).unwrap();
        symlink("/dev/null", base.join("fd/0")).unwrap();
        for (i, inode) in inodes.iter().enumerate() {
            symlink(
                format!("socket:[{inode}]"),
                base.join(format!("fd/{}", i + 3)),
            )
            .unwrap();
        }
    }

    #[cfg(target_endian = "little")]
    #[test]
    fn finds_project_servers_from_proc_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proc");
        let project = dir.path().join("app");
        let other = dir.path().join("elsewhere");
        fs::create_dir_all(root.join("net")).unwrap();
        fs::create_dir_all(project.join("web")).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(root.join("net/tcp"), TCP).unwrap();
        fs::write(root.join("net/tcp6"), TCP6).unwrap();
        // Vite in a subfolder of the project: 127.0.0.1:5173 and [::1]:4000.
        process(
            &root,
            100,
            &project.join("web"),
            "node",
            "node vite --port 5173",
            &[41001, 42001, 999],
        );
        // A server bound only to a LAN address: not reachable over loopback.
        process(
            &root,
            101,
            &project,
            "python3",
            "python3 -m http.server",
            &[41003],
        );
        // Outside the project.
        process(&root, 102, &other, "postgres", "postgres", &[41002]);
        // ShadowCode itself.
        process(&root, 103, &project, "shadowcode", "shadowcode", &[41005]);
        // Not a number: ignored.
        fs::create_dir_all(root.join("self")).unwrap();
        let found = project_servers(&root, &project, 103);
        let ports: Vec<(u16, u32)> = found.iter().map(|f| (f.port, f.pid)).collect();
        assert_eq!(ports, vec![(4000, 100), (5173, 100)]);
        assert_eq!(found[1].process, "node");
        assert_eq!(found[1].command, "node vite --port 5173");
        assert_eq!(own_ports(&root, 103), HashSet::from([9000]));
        assert_eq!(own_ports(&root, 100), HashSet::from([5173, 4000]));
        assert!(project_servers(&dir.path().join("missing"), &project, 0).is_empty());
    }

    #[test]
    fn reads_urls_from_dev_server_output() {
        let vite = "\x1b[32m  VITE v7.1.5\x1b[39m  ready in 300 ms\n\n  ➜  \x1b[1mLocal\x1b[22m:   \x1b[36mhttp://localhost:\x1b[1m5173\x1b[22m/\x1b[39m\n  ➜  Network: http://192.168.1.5:5173/\n";
        assert_eq!(
            printed_urls(vite),
            vec![PrintedUrl {
                url: "http://localhost:5173/".into(),
                port: 5173
            }]
        );
        let next = "ready - started server on 0.0.0.0:3000, url: http://0.0.0.0:3000.\nAlso http://127.0.0.1:3000/again and (http://app.localhost:8787/api/x) and http://[::1]:4000 and http://example.com:80/";
        assert_eq!(
            printed_urls(next),
            vec![
                PrintedUrl {
                    url: "http://localhost:3000/".into(),
                    port: 3000
                },
                PrintedUrl {
                    url: "http://app.localhost:8787/api/x".into(),
                    port: 8787
                },
                PrintedUrl {
                    url: "http://[::1]:4000/".into(),
                    port: 4000
                },
            ]
        );
        assert!(printed_urls("http://localhost:99999/ http://localhost/").is_empty());
    }
}
