//! System pressure and the processes MonoCode owns, extracted from
//! src-tauri/src/resources.rs and resourcePressure.ts.
use crate::process::{self, Agent};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mach_host_self() -> u32;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub cpu_percent: Option<f64>,
    pub memory_pressure: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub usage: Usage,
    /// The share of the last interval the UI thread spent late.
    pub stall_share: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Cpu,
    Memory,
    Stalls,
}
impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Reason::Cpu => "High CPU usage",
            Reason::Memory => "Low available memory",
            Reason::Stalls => "Slow response",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pressure {
    pub active: bool,
    high: u32,
    low: u32,
    last_notice: Option<Instant>,
    pub reasons: Vec<Reason>,
}
impl Pressure {
    /// Three high samples to enter, six healthy samples to recover, 60s cooldown.
    pub fn sample(&mut self, sample: Sample, now: Instant) {
        let cpu = sample.usage.cpu_percent.unwrap_or(0.);
        let memory = sample.usage.memory_pressure.unwrap_or(0);
        let reasons: Vec<_> = [
            (cpu >= 85.).then_some(Reason::Cpu),
            (memory >= 2).then_some(Reason::Memory),
            (sample.stall_share >= 0.1).then_some(Reason::Stalls),
        ]
        .into_iter()
        .flatten()
        .collect();
        let healthy = cpu < 65. && memory < 2 && sample.stall_share < 0.03;
        self.high = if reasons.is_empty() { 0 } else { self.high + 1 };
        self.low = if healthy { self.low + 1 } else { 0 };
        let cooled = self
            .last_notice
            .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(60));
        if !self.active && self.high >= 3 && cooled {
            self.active = true;
            self.last_notice = Some(now);
            self.reasons = reasons;
        } else if self.active && self.low >= 6 {
            self.active = false;
            self.high = 0;
            self.reasons.clear();
        } else if self.active && !reasons.is_empty() {
            self.reasons = reasons;
        }
    }
    /// A hidden window's timers stall by design; start counting afresh.
    pub fn pause(&mut self) {
        self.high = 0;
        self.low = 0;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OwnedProcess {
    pub pid: u32,
    pub identity: String,
    pub session: i64,
    pub command: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    /// The provider process itself, rather than a tool it started.
    pub agent: bool,
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
pub fn identity(pid: u32) -> Option<String> {
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
pub fn identity(_pid: u32) -> Option<String> {
    None
}

/// System CPU since the previous call, and the kernel's memory pressure level.
pub fn usage() -> Usage {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::sync::{Mutex, OnceLock};
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
        Usage {
            cpu_percent,
            memory_pressure: ok.then_some(pressure),
        }
    }
    #[cfg(not(target_os = "macos"))]
    Usage::default()
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

fn owner<'a>(pid: u32, processes: &HashMap<u32, Process>, roots: &'a [Agent]) -> Option<&'a Agent> {
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

fn verified_roots() -> Vec<Agent> {
    process::agents()
        .into_iter()
        .filter(|root| root.identity.is_some() && identity(root.pid) == root.identity)
        .collect()
}

/// Every live provider process and its descendants, busiest first. Blocking.
pub fn processes() -> Result<Vec<OwnedProcess>, String> {
    let roots = verified_roots();
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let processes = process_snapshot()?;
    Ok(owned(&processes, &roots, identity))
}

fn owned(
    processes: &HashMap<u32, Process>,
    roots: &[Agent],
    identity: impl Fn(u32) -> Option<String>,
) -> Vec<OwnedProcess> {
    let mut owned: Vec<OwnedProcess> = processes
        .values()
        .filter_map(|process| {
            let root = owner(process.pid, processes, roots)?;
            Some(OwnedProcess {
                pid: process.pid,
                identity: identity(process.pid)?,
                session: root.session,
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
    owned
}

fn validate_tool<'a>(
    pid: u32,
    stamp: &str,
    processes: &HashMap<u32, Process>,
    roots: &'a [Agent],
) -> Result<&'a Agent, String> {
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

/// Called only by the user's Stop action for a build or tool, never by
/// pressure detection. The agent keeps running. Blocking.
pub fn stop_tool(pid: u32, stamp: &str) -> Result<(), String> {
    stop_tool_in(pid, stamp, &verified_roots())
}

fn stop_tool_in(pid: u32, stamp: &str, roots: &[Agent]) -> Result<(), String> {
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
        std::thread::sleep(Duration::from_secs(2));
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

    fn high() -> Sample {
        Sample {
            usage: Usage {
                cpu_percent: Some(95.),
                memory_pressure: Some(1),
            },
            stall_share: 0.,
        }
    }

    #[test]
    fn pressure_needs_three_high_samples_six_healthy_and_a_cooldown() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let mut pressure = Pressure::default();
        pressure.sample(high(), at(0));
        pressure.sample(high(), at(3));
        assert!(!pressure.active);
        pressure.sample(high(), at(6));
        assert!(pressure.active);
        assert_eq!(pressure.reasons, [Reason::Cpu]);
        // In between, neither high nor healthy: the notice stays.
        let middling = Sample {
            usage: Usage {
                cpu_percent: Some(70.),
                memory_pressure: None,
            },
            stall_share: 0.,
        };
        pressure.sample(middling, at(9));
        assert!(pressure.active);
        for s in 0..6 {
            pressure.sample(Sample::default(), at(12 + s * 3));
        }
        assert!(!pressure.active);
        assert!(pressure.reasons.is_empty());
        // Within the 60s cooldown, saturation does not raise it again.
        for s in 0..3 {
            pressure.sample(high(), at(30 + s * 3));
        }
        assert!(!pressure.active);
        pressure.sample(high(), at(66));
        assert!(pressure.active);
    }

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
        let roots = [Agent {
            session: 7,
            pid: 10,
            identity: Some("10:1:1".into()),
        }];
        assert_eq!(owner(12, &processes, &roots).unwrap().session, 7);
        assert!(owner(20, &processes, &roots).is_none());
        assert!(owner(1, &processes, &roots).is_none());
        assert!(validate_tool(10, "10:1:1", &processes, &roots).is_err());
        assert!(validate_tool(12, "stale", &processes, &roots).is_err());
        let listed = owned(&processes, &roots, |pid| Some(pid.to_string()));
        assert_eq!(
            listed.iter().map(|p| (p.pid, p.agent)).collect::<Vec<_>>(),
            [(11, false), (12, false), (10, true)]
        );
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
        let root = Agent {
            session: 1,
            pid: root_pid,
            identity: identity(root_pid),
        };
        let mut tool = Child(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        let mut sibling = Child(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        let pid = tool.0.id();
        let stamp = identity(pid).unwrap();
        assert!(stop_tool_in(pid, "stale identity", std::slice::from_ref(&root)).is_err());
        assert!(tool.0.try_wait().unwrap().is_none());
        stop_tool_in(pid, &stamp, std::slice::from_ref(&root)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while tool.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(tool.0.try_wait().unwrap().is_some());
        assert!(sibling.0.try_wait().unwrap().is_none());
        assert_eq!(identity(root_pid), root.identity);
    }

    #[cfg(unix)]
    #[test]
    fn owned_children_are_listed_under_their_session() {
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let registration = process::owned_by(42, || process::register(child.id()));
        let agents = process::agents();
        let mine = agents.iter().find(|a| a.pid == child.id()).unwrap();
        assert_eq!(mine.session, 42);
        drop(registration);
        assert!(process::agents().iter().all(|a| a.pid != child.id()));
        let _ = child.kill();
        let _ = child.wait();
    }
}
