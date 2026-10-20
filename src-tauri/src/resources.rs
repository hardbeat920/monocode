use crate::harness::HarnessHost;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::{Mutex, OnceLock};
use tauri::State;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mach_host_self() -> u32;
}

#[derive(Clone)]
pub(crate) struct ProcessRoot {
    pub session_id: String,
    pub pid: u32,
    pub identity: Option<String>,
    pub cwd: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    cpu_percent: Option<f64>,
    memory_pressure: Option<u32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedProcess {
    pid: u32,
    identity: String,
    session_id: String,
    cwd: String,
    command: String,
    cpu_percent: f64,
    memory_bytes: u64,
    agent: bool,
}

#[derive(Clone)]
struct Process {
    pid: u32,
    parent: u32,
    command: String,
    cpu: f64,
    memory: u64,
}

/// Kernel process birth time prevents PID reuse from authorizing a stop.
#[cfg(target_os = "macos")]
pub(crate) fn identity(pid: u32) -> Option<String> {
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = std::mem::size_of_val(&info) as i32;
        if libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut _,
            size,
        ) != size
        {
            return None;
        }
        Some(format!(
            "{}:{}:{}",
            pid, info.pbi_start_tvsec, info.pbi_start_tvusec
        ))
    }
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn identity(_pid: u32) -> Option<String> {
    None
}

#[tauri::command]
pub fn resource_usage() -> ResourceUsage {
    #[cfg(target_os = "macos")]
    unsafe {
        static CPU: Mutex<Option<[u32; 4]>> = Mutex::new(None);
        static HOST: OnceLock<u32> = OnceLock::new();
        let host = *HOST.get_or_init(|| mach_host_self());
        let mut ticks = [0u32; 4];
        let mut count = 4;
        let mut cpu_percent = None;
        if libc::host_statistics(
            host,
            libc::HOST_CPU_LOAD_INFO,
            ticks.as_mut_ptr() as *mut i32,
            &mut count,
        ) == 0
        {
            let mut previous = CPU.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(before) = *previous {
                let delta: Vec<u64> = ticks
                    .iter()
                    .zip(before)
                    .map(|(now, old)| now.wrapping_sub(old) as u64)
                    .collect();
                let total: u64 = delta.iter().sum();
                if total > 0 {
                    cpu_percent = Some(100.0 * (total - delta[2]) as f64 / total as f64);
                }
            }
            *previous = Some(ticks);
        }
        let mut pressure: u32 = 0;
        let mut length = std::mem::size_of_val(&pressure);
        let ok = libc::sysctlbyname(
            c"kern.memorystatus_vm_pressure_level".as_ptr(),
            &mut pressure as *mut _ as *mut _,
            &mut length,
            std::ptr::null_mut(),
            0,
        ) == 0;
        ResourceUsage {
            cpu_percent,
            memory_pressure: ok.then_some(pressure),
        }
    }
    #[cfg(not(target_os = "macos"))]
    ResourceUsage {
        cpu_percent: None,
        memory_pressure: None,
    }
}

fn process_snapshot() -> Result<HashMap<u32, Process>, String> {
    let output = Command::new("/bin/ps")
        .env("LC_ALL", "C")
        .args(["-axww", "-o", "pid=,ppid=,%cpu=,rss=,command="])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("Could not read process usage".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_process)
        .map(|process| (process.pid, process))
        .collect())
}

fn parse_process(line: &str) -> Option<Process> {
    let mut fields = line.split_whitespace();
    Some(Process {
        pid: fields.next()?.parse().ok()?,
        parent: fields.next()?.parse().ok()?,
        cpu: fields.next()?.parse().ok()?,
        memory: fields.next()?.parse::<u64>().ok()?.saturating_mul(1024),
        command: fields.collect::<Vec<_>>().join(" "),
    })
}

fn owner<'a>(
    pid: u32,
    processes: &HashMap<u32, Process>,
    roots: &'a [ProcessRoot],
) -> Option<&'a ProcessRoot> {
    let mut seen = HashSet::new();
    let mut current = pid;
    while current > 1 && seen.insert(current) {
        if let Some(root) = roots
            .iter()
            .find(|root| root.pid == current && root.identity.is_some())
        {
            return Some(root);
        }
        current = processes.get(&current)?.parent;
    }
    None
}

fn verified_roots(host: &HarnessHost) -> Vec<ProcessRoot> {
    host.process_roots()
        .into_iter()
        .filter(|root| root.identity.is_some() && identity(root.pid) == root.identity)
        .collect()
}

#[tauri::command]
pub async fn resource_processes(host: State<'_, HarnessHost>) -> Result<Vec<OwnedProcess>, String> {
    let roots = verified_roots(&host);
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    crate::background_work::read(move || {
        let processes = process_snapshot()?;
        let mut owned: Vec<OwnedProcess> = processes
            .values()
            .filter_map(|process| {
                let root = owner(process.pid, &processes, &roots)?;
                Some(OwnedProcess {
                    pid: process.pid,
                    identity: identity(process.pid)?,
                    session_id: root.session_id.clone(),
                    cwd: root.cwd.clone(),
                    command: process.command.clone(),
                    cpu_percent: process.cpu,
                    memory_bytes: process.memory,
                    agent: process.pid == root.pid,
                })
            })
            .collect();
        owned.sort_by(|a, b| {
            b.cpu_percent
                .total_cmp(&a.cpu_percent)
                .then(b.memory_bytes.cmp(&a.memory_bytes))
        });
        Ok(owned)
    })
    .await?
}

fn validate_tool<'a>(
    pid: u32,
    stamp: &str,
    processes: &HashMap<u32, Process>,
    roots: &'a [ProcessRoot],
) -> Result<&'a ProcessRoot, String> {
    let root = owner(pid, processes, roots).ok_or("This process is no longer owned by MonoCode")?;
    if identity(root.pid) != root.identity {
        return Err("The owning agent has exited or changed".into());
    }
    if root.pid == pid {
        return Err("Use Stop agent for the agent process".into());
    }
    if identity(pid).as_deref() != Some(stamp) {
        return Err("This process has exited or changed; refresh the list".into());
    }
    if !live_descendant(pid, root.pid, root.identity.as_deref().unwrap_or_default()) {
        return Err("This process is no longer owned by MonoCode".into());
    }
    Ok(root)
}

// A ps snapshot is for display, not authority to signal a PID. Walk current
// kernel parent links as well, so a reused descendant PID cannot be targeted.
#[cfg(target_os = "macos")]
fn live_descendant(pid: u32, ancestor: u32, stamp: &str) -> bool {
    let mut current = pid;
    let mut seen = HashSet::new();
    while current > 1 && seen.insert(current) {
        if current == ancestor {
            return identity(ancestor).as_deref() == Some(stamp);
        }
        unsafe {
            let mut info: libc::proc_bsdinfo = std::mem::zeroed();
            let size = std::mem::size_of_val(&info) as i32;
            if libc::proc_pidinfo(
                current as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut _,
                size,
            ) != size
            {
                return false;
            }
            current = info.pbi_ppid;
        }
    }
    false
}
#[cfg(not(target_os = "macos"))]
fn live_descendant(_pid: u32, _ancestor: u32, _stamp: &str) -> bool {
    false
}

/// Called only by the user's Stop build/tool action. Never by pressure detection.
#[tauri::command]
pub async fn resource_stop_tool(
    host: State<'_, HarnessHost>,
    pid: u32,
    identity: String,
) -> Result<(), String> {
    let roots = verified_roots(&host);
    // A user's stop request must not queue behind optional Git/index reads.
    tauri::async_runtime::spawn_blocking(move || stop_tool(pid, &identity, &roots))
        .await
        .map_err(|error| error.to_string())?
}

fn stop_tool(pid: u32, stamp: &str, roots: &[ProcessRoot]) -> Result<(), String> {
    let processes = process_snapshot()?;
    let root = validate_tool(pid, stamp, &processes, roots)?;
    let mut subtree = HashSet::from([pid]);
    loop {
        let before = subtree.len();
        for process in processes.values() {
            if subtree.contains(&process.parent)
                && owner(process.pid, &processes, roots).is_some_and(|entry| entry.pid == root.pid)
                && process.pid != root.pid
            {
                subtree.insert(process.pid);
            }
        }
        if before == subtree.len() {
            break;
        }
    }
    let targets: Vec<_> = subtree
        .into_iter()
        .filter_map(|candidate| {
            let birth = identity(candidate)?;
            if !live_descendant(candidate, pid, stamp)
                || identity(candidate).as_ref() != Some(&birth)
            {
                return None;
            }
            Some((candidate, birth))
        })
        .collect();
    #[cfg(unix)]
    for (pid, stamp) in &targets {
        if identity(*pid).as_ref() == Some(stamp) {
            unsafe {
                libc::kill(*pid as i32, libc::SIGTERM);
            }
        }
    }
    // Escalation belongs to this explicit stop request; recheck birth times.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(2));
        #[cfg(not(unix))]
        let _ = targets;
        #[cfg(unix)]
        for (pid, stamp) in targets {
            if identity(pid).as_ref() == Some(&stamp) {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ancestry_never_includes_unrelated_or_ancestor_processes() {
        let rows = [
            "10 1 0.0 100 agent",
            "11 10 120.0 2048 cargo build",
            "12 11 100.0 4096 rustc module.rs",
            "20 1 10.0 100 unrelated",
        ];
        let processes = rows
            .iter()
            .filter_map(|row| parse_process(row))
            .map(|row| (row.pid, row))
            .collect();
        let roots = [ProcessRoot {
            session_id: "owned".into(),
            pid: 10,
            identity: Some("10:1:1".into()),
            cwd: "/worktree".into(),
        }];
        assert_eq!(owner(12, &processes, &roots).unwrap().session_id, "owned");
        assert!(owner(20, &processes, &roots).is_none());
        assert!(owner(1, &processes, &roots).is_none());
        assert!(validate_tool(10, "10:1:1", &processes, &roots).is_err());
        assert!(validate_tool(12, "stale", &processes, &roots).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn stopping_a_tool_preserves_its_agent_and_sibling() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let root_pid = std::process::id();
        let root = ProcessRoot {
            session_id: "test".into(),
            pid: root_pid,
            identity: identity(root_pid),
            cwd: "/tmp".into(),
        };
        let mut tool = Child(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        let mut sibling = Child(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        let pid = tool.0.id();
        let stamp = identity(pid).unwrap();
        assert!(stop_tool(pid, "stale identity", std::slice::from_ref(&root)).is_err());
        assert!(tool.0.try_wait().unwrap().is_none());
        stop_tool(pid, &stamp, std::slice::from_ref(&root)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while tool.0.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(tool.0.try_wait().unwrap().is_some());
        assert!(sibling.0.try_wait().unwrap().is_none());
        assert_eq!(identity(root_pid), root.identity);
    }
}
