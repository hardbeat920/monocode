use super::Profile;
use crate::provider::Kind;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
type Children = HashMap<u32, (Kind, Profile, Option<String>)>;
fn children() -> &'static Mutex<Children> {
    static CHILDREN: OnceLock<Mutex<Children>> = OnceLock::new();
    CHILDREN.get_or_init(Default::default)
}
pub(crate) struct Tracked(u32);
pub(crate) fn track(pid: u32, kind: Kind, profile: &Profile) -> Tracked {
    children().lock().unwrap_or_else(|e| e.into_inner()).insert(
        pid,
        (kind, profile.clone(), crate::resources::identity(pid)),
    );
    Tracked(pid)
}
impl Drop for Tracked {
    fn drop(&mut self) {
        children()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}
/// Run on a worker before logout/removal. Transport workers reap the children.
pub fn stop_processes(kind: Kind, profile: &Profile) {
    let targets: Vec<_> = children()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(_, (k, p, _))| *k == kind && p == profile)
        .map(|(pid, (_, _, id))| (*pid, id.clone()))
        .collect();
    for (pid, identity) in targets {
        if identity.is_some() && crate::resources::identity(pid) != identity {
            continue;
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(0x0800_0000)
                .status();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while children()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .any(|(k, p, _)| *k == kind && p == profile)
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
}
