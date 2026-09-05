use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::sync::OnceLock;
use windows_sys::Win32::System::{
    JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    },
    LibraryLoader::{
        SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_APPLICATION_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
    },
};

static MANAGED_JOB: OnceLock<Result<OwnedHandle, i32>> = OnceLock::new();

/// Restrict DLL lookup before the first PTY is opened. MonoCode itself stays
/// outside the job so relaunches and external applications do not inherit it.
pub(crate) fn initialize() -> io::Result<()> {
    unsafe {
        if SetDefaultDllDirectories(
            LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    managed_job().map(|_| ())
}

fn managed_job() -> io::Result<&'static OwnedHandle> {
    MANAGED_JOB
        .get_or_init(|| create_job().map_err(|err| err.raw_os_error().unwrap_or(1)))
        .as_ref()
        .map_err(|code| io::Error::from_raw_os_error(*code))
}

fn create_job() -> io::Result<OwnedHandle> {
    unsafe {
        let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = OwnedHandle::from_raw_handle(raw);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }
}

/// The app owns the only job handle. OS handle cleanup kills registered trees
/// after a crash; unrelated children are never enrolled in this job.
pub(crate) fn assign_child(process: RawHandle) -> io::Result<()> {
    if unsafe { AssignProcessToJobObject(managed_job()?.as_raw_handle(), process) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn spawn_managed(
    command: &mut std::process::Command,
) -> io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    };
    managed_job()?;
    // Probes can exit or create descendants before spawn returns. Enroll them
    // while suspended, then let the first thread run.
    command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
    let mut child = command.spawn()?;
    if let Err(err) = assign_child(child.as_raw_handle()).and_then(|()| resume_child(child.id())) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }
    Ok(child)
}

fn resume_child(pid: u32) -> io::Result<()> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
    };
    // Stable Rust does not expose Child's primary-thread handle. The suspended
    // process has not run user code, so find that thread in the OS snapshot.
    unsafe {
        let raw = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = OwnedHandle::from_raw_handle(raw);
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut found = Thread32First(snapshot.as_raw_handle(), &mut entry);
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                let raw = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if raw.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = OwnedHandle::from_raw_handle(raw);
                if ResumeThread(thread.as_raw_handle()) == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            found = Thread32Next(snapshot.as_raw_handle(), &mut entry);
        }
        Err(io::Error::other("Managed child has no primary thread"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, TerminateProcess, WaitForSingleObject,
    };

    #[test]
    fn managed_short_lived_command_preserves_exit_status() {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit", "7"]);
        assert_eq!(
            spawn_managed(&mut command).unwrap().wait().unwrap().code(),
            Some(7)
        );
    }

    #[test]
    // This subprocess deliberately dies before it can wait; the parent verifies cleanup.
    #[allow(clippy::zombie_processes)]
    fn abrupt_exit_kills_children() {
        const MARKER: &str = "MONOCODE_JOB_TEST_CHILD";
        if std::env::var_os(MARKER).is_some() {
            initialize().unwrap();
            let mut command = Command::new("ping.exe");
            crate::hide_window_console(&mut command);
            command
                .args(["-n", "60", "127.0.0.1"])
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let child = spawn_managed(&mut command).unwrap();
            crate::hide_window_console(&mut command);
            let external = command.spawn().unwrap();
            let pair = portable_pty::native_pty_system()
                .openpty(portable_pty::PtySize::default())
                .unwrap();
            let mut terminal = portable_pty::CommandBuilder::new("cmd.exe");
            terminal.arg("/D");
            let terminal = pair.slave.spawn_command(terminal).unwrap();
            assign_child(terminal.as_raw_handle().unwrap()).unwrap();
            println!("CHILD_PID={}", child.id());
            println!("TERMINAL_PID={}", terminal.process_id().unwrap());
            println!("EXTERNAL_PID={}", external.id());
            std::io::stdout().flush().unwrap();
            // Skip every Rust destructor, as a forced termination would.
            unsafe {
                TerminateProcess(GetCurrentProcess(), 0);
            }
            unreachable!();
        }
        let mut parent = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "windows::tests::abrupt_exit_kills_children",
                "--nocapture",
            ])
            .env(MARKER, "1")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        // Stop at the explicit marker: an ordinary child may still hold an
        // inherited pipe handle after the helper process has exited.
        let mut stdout = String::new();
        for line in BufReader::new(parent.stdout.take().unwrap()).lines() {
            let line = line.unwrap();
            stdout.push_str(&line);
            stdout.push('\n');
            if line.starts_with("EXTERNAL_PID=") {
                break;
            }
        }
        assert!(parent.wait().unwrap().success());
        let external_pid: u32 = stdout
            .lines()
            .find_map(|line| line.strip_prefix("EXTERNAL_PID="))
            .expect("external process was started")
            .parse()
            .unwrap();
        unsafe {
            // The relaunch/external-app spawn path must survive parent exit.
            let raw = OpenProcess(0x0010_0001, 0, external_pid); // SYNCHRONIZE | TERMINATE
            assert!(!raw.is_null());
            let external = OwnedHandle::from_raw_handle(raw);
            let status = WaitForSingleObject(external.as_raw_handle(), 0);
            TerminateProcess(external.as_raw_handle(), 0);
            WaitForSingleObject(external.as_raw_handle(), 5000);
            assert_eq!(status, 258); // WAIT_TIMEOUT: still running
        }
        for prefix in ["CHILD_PID=", "TERMINAL_PID="] {
            let pid: u32 = stdout
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .expect("child process was started")
                .parse()
                .unwrap();
            unsafe {
                let raw = OpenProcess(0x0010_0000, 0, pid); // SYNCHRONIZE
                if raw.is_null() {
                    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(87));
                } else {
                    let process = OwnedHandle::from_raw_handle(raw);
                    assert_eq!(WaitForSingleObject(process.as_raw_handle(), 5000), 0);
                }
            }
        }
    }
}
