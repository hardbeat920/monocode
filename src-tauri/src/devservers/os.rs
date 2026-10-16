//! What the operating system says about listening ports and processes.
//! On macOS and Linux every function answers for the current user's
//! processes only; on Windows the tables cover every user. Every function
//! returns an empty answer rather than an error: a missing dev server is the
//! worst outcome of a failed read.

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

/// A TCP port in LISTEN state that a browser on this machine can reach as
/// `localhost` (bound to loopback or to every address).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    pub port: u16,
    pub pid: u32,
    /// Bound on IPv6 only (`::1`), so a probe has to use `[::1]`.
    pub v6_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proc {
    pub ppid: u32,
    /// Process group (Unix); `None` where the OS has none.
    pub pgid: Option<u32>,
    pub name: String,
}

fn reachable_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_unspecified()
}

fn reachable_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || ip.to_ipv4_mapped().is_some_and(reachable_v4)
}

/// Merge v4 and v6 rows of one port and process into one listener.
fn merge(rows: Vec<(u16, u32, bool)>) -> Vec<Listener> {
    let mut by_key: HashMap<(u16, u32), bool> = HashMap::new();
    for (port, pid, v6) in rows {
        if port == 0 || pid == 0 {
            continue;
        }
        let v6_only = by_key.entry((port, pid)).or_insert(v6);
        *v6_only &= v6;
    }
    let mut listeners: Vec<Listener> = by_key
        .into_iter()
        .map(|((port, pid), v6_only)| Listener { port, pid, v6_only })
        .collect();
    listeners.sort_by_key(|listener| (listener.port, listener.pid));
    listeners
}

// ---------------------------------------------------------------------------
// Linux: /proc
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub fn listeners() -> Vec<Listener> {
    let mut sockets: HashMap<u64, (u16, bool)> = HashMap::new();
    for (file, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        if let Ok(text) = std::fs::read_to_string(file) {
            for (inode, port) in parse_proc_net_tcp(&text, v6) {
                sockets.insert(inode, (port, v6));
            }
        }
    }
    if sockets.is_empty() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for pid in proc_pids() {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };
            let target = target.to_string_lossy();
            let Some(inode) = target
                .strip_prefix("socket:[")
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|inode| inode.parse::<u64>().ok())
            else {
                continue;
            };
            if let Some(&(port, v6)) = sockets.get(&inode) {
                rows.push((port, pid, v6));
            }
        }
    }
    merge(rows)
}

/// `(inode, port)` of the LISTEN rows of `/proc/net/tcp` or `tcp6` that
/// localhost reaches.
#[cfg(any(target_os = "linux", test))]
pub fn parse_proc_net_tcp(text: &str, v6: bool) -> Vec<(u64, u16)> {
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // sl local rem st tx:rx tr:when retrnsmt uid timeout inode
        if fields.len() < 10 || fields[3] != "0A" {
            continue;
        }
        let Some((addr, port)) = fields[1].split_once(':') else {
            continue;
        };
        let Ok(port) = u16::from_str_radix(port, 16) else {
            continue;
        };
        let reachable = if v6 {
            parse_hex_v6(addr).is_some_and(reachable_v6)
        } else {
            u32::from_str_radix(addr, 16)
                .ok()
                .map(|word| Ipv4Addr::from(word.to_le_bytes()))
                .is_some_and(reachable_v4)
        };
        if !reachable {
            continue;
        }
        if let Ok(inode) = fields[9].parse::<u64>() {
            if inode != 0 {
                rows.push((inode, port));
            }
        }
    }
    rows
}

/// The kernel prints an IPv6 address as four 32-bit words in host order.
#[cfg(any(target_os = "linux", test))]
fn parse_hex_v6(hex: &str) -> Option<Ipv6Addr> {
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for word in 0..4 {
        let value = u32::from_str_radix(&hex[word * 8..word * 8 + 8], 16).ok()?;
        bytes[word * 4..word * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Some(Ipv6Addr::from(bytes))
}

#[cfg(target_os = "linux")]
fn proc_pids() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .collect()
}

#[cfg(target_os = "linux")]
pub fn processes() -> HashMap<u32, Proc> {
    proc_pids()
        .into_iter()
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            Some((pid, parse_proc_stat(&stat)?))
        })
        .collect()
}

/// `pid (comm) state ppid pgrp ...`; `comm` may hold spaces and parentheses.
#[cfg(any(target_os = "linux", test))]
pub fn parse_proc_stat(stat: &str) -> Option<Proc> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let name = stat.get(open + 1..close)?.to_string();
    let mut rest = stat.get(close + 1..)?.split_whitespace();
    let _state = rest.next()?;
    let ppid = rest.next()?.parse().ok()?;
    let pgid = rest.next()?.parse().ok()?;
    Some(Proc {
        ppid,
        pgid: Some(pgid),
        name,
    })
}

#[cfg(target_os = "linux")]
pub fn cwds(pids: &HashSet<u32>) -> HashMap<u32, PathBuf> {
    pids.iter()
        .filter_map(|pid| Some((*pid, std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?)))
        .collect()
}

// ---------------------------------------------------------------------------
// macOS: lsof and ps, both part of the base system
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn run(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
pub fn listeners() -> Vec<Listener> {
    parse_lsof_listen(&run(
        "/usr/sbin/lsof",
        &["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"],
    ))
}

/// `lsof -Fpn` output: `p<pid>` starts a process, `n<addr>:<port>` names
/// each of its sockets (`*:5173`, `127.0.0.1:5173`, `[::1]:5173`).
#[cfg(any(target_os = "macos", test))]
pub fn parse_lsof_listen(text: &str) -> Vec<Listener> {
    let mut rows = Vec::new();
    let mut pid = 0u32;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix('p') {
            pid = value.trim().parse().unwrap_or(0);
        } else if let Some(name) = line.strip_prefix('n') {
            let Some((host, port)) = name.rsplit_once(':') else {
                continue;
            };
            let Ok(port) = port.trim().parse::<u16>() else {
                continue;
            };
            let (reachable, v6) = if host == "*" {
                (true, false)
            } else if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
                let inner = inner.split('%').next().unwrap_or(inner);
                (inner.parse::<Ipv6Addr>().is_ok_and(reachable_v6), true)
            } else {
                (host.parse::<Ipv4Addr>().is_ok_and(reachable_v4), false)
            };
            if reachable {
                rows.push((port, pid, v6));
            }
        }
    }
    merge(rows)
}

#[cfg(target_os = "macos")]
pub fn processes() -> HashMap<u32, Proc> {
    parse_ps(&run("/bin/ps", &["-axo", "pid=,ppid=,pgid=,comm="]))
}

/// `ps -axo pid=,ppid=,pgid=,comm=`; `comm` is a path that may hold spaces.
#[cfg(any(target_os = "macos", test))]
pub fn parse_ps(text: &str) -> HashMap<u32, Proc> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let pgid = fields.next()?.parse().ok()?;
            let comm = fields.collect::<Vec<_>>().join(" ");
            let name = comm.rsplit('/').next().unwrap_or(&comm).to_string();
            Some((
                pid,
                Proc {
                    ppid,
                    pgid: Some(pgid),
                    name,
                },
            ))
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub fn cwds(pids: &HashSet<u32>) -> HashMap<u32, PathBuf> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let text = run("/usr/sbin/lsof", &["-a", "-d", "cwd", "-p", &list, "-Fpn"]);
    let mut cwds = HashMap::new();
    let mut pid = 0u32;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix('p') {
            pid = value.trim().parse().unwrap_or(0);
        } else if let Some(path) = line.strip_prefix('n') {
            if pid != 0 {
                cwds.insert(pid, PathBuf::from(path));
            }
        }
    }
    cwds
}

// ---------------------------------------------------------------------------
// Windows: IP Helper and Toolhelp
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub fn listeners() -> Vec<Listener> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    const NO_ERROR: u32 = 0;

    /// The table as `u32` words: aligned for its rows, sized by the OS.
    fn table(family: u32) -> Option<Vec<u32>> {
        let mut size = 0u32;
        let mut buffer: Vec<u32> = Vec::new();
        // The table can grow between the size query and the read.
        for _ in 0..4 {
            let status = unsafe {
                GetExtendedTcpTable(
                    if buffer.is_empty() {
                        std::ptr::null_mut()
                    } else {
                        buffer.as_mut_ptr().cast()
                    },
                    &mut size,
                    0,
                    family,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if status == NO_ERROR && !buffer.is_empty() {
                return Some(buffer);
            }
            buffer = vec![0u32; (size as usize).div_ceil(4) + 16];
            size = (buffer.len() * 4) as u32;
        }
        None
    }

    /// The port is in network byte order in the low 16 bits.
    fn port(raw: u32) -> u16 {
        u16::from_be(raw as u16)
    }

    let mut rows = Vec::new();
    if let Some(buffer) = table(AF_INET) {
        let count = buffer[0] as usize;
        let first = unsafe { buffer.as_ptr().add(1).cast::<MIB_TCPROW_OWNER_PID>() };
        let room = (buffer.len() * 4 - 4) / std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        for index in 0..count.min(room) {
            let row = unsafe { std::ptr::read_unaligned(first.add(index)) };
            let ip = Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes());
            if reachable_v4(ip) {
                rows.push((port(row.dwLocalPort), row.dwOwningPid, false));
            }
        }
    }
    if let Some(buffer) = table(AF_INET6) {
        let count = buffer[0] as usize;
        let first = unsafe { buffer.as_ptr().add(1).cast::<MIB_TCP6ROW_OWNER_PID>() };
        let room = (buffer.len() * 4 - 4) / std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
        for index in 0..count.min(room) {
            let row = unsafe { std::ptr::read_unaligned(first.add(index)) };
            if reachable_v6(Ipv6Addr::from(row.ucLocalAddr)) {
                rows.push((port(row.dwLocalPort), row.dwOwningPid, true));
            }
        }
    }
    merge(rows)
}

#[cfg(windows)]
pub fn processes() -> HashMap<u32, Proc> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut processes = HashMap::new();
    unsafe {
        let raw = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if raw == INVALID_HANDLE_VALUE {
            return processes;
        }
        let snapshot = OwnedHandle::from_raw_handle(raw);
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut found = Process32FirstW(snapshot.as_raw_handle(), &mut entry);
        while found != 0 {
            let len = entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
            let name = exe
                .strip_suffix(".exe")
                .or_else(|| exe.strip_suffix(".EXE"))
                .unwrap_or(&exe)
                .to_string();
            processes.insert(
                entry.th32ProcessID,
                Proc {
                    ppid: entry.th32ParentProcessID,
                    pgid: None,
                    name,
                },
            );
            found = Process32NextW(snapshot.as_raw_handle(), &mut entry);
        }
    }
    processes
}

/// Windows keeps a process's working directory in its own memory; MonoCode
/// finds Windows dev servers through the process tree instead.
#[cfg(windows)]
pub fn cwds(_pids: &HashSet<u32>) -> HashMap<u32, PathBuf> {
    HashMap::new()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn listeners() -> Vec<Listener> {
    Vec::new()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn processes() -> HashMap<u32, Proc> {
    HashMap::new()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn cwds(_pids: &HashSet<u32>) -> HashMap<u32, PathBuf> {
    HashMap::new()
}
