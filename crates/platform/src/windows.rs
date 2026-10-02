use std::io;
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
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

pub(crate) fn spawn_pty(
    slave: &dyn portable_pty::SlavePty,
    command: portable_pty::CommandBuilder,
) -> Result<Box<dyn portable_pty::Child + Send + Sync>, String> {
    let job = managed_job().map_err(|err| err.to_string())?;
    slave
        .spawn_command_in_job(command, job.as_handle())
        .map_err(|err| err.to_string())
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
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, TerminateProcess, WaitForSingleObject,
        PROCESS_QUERY_INFORMATION,
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
    fn pty_rejects_an_invalid_job_without_unprotected_fallback() {
        initialize().unwrap();
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize::default())
            .unwrap();
        // A valid handle of the wrong object type must fail process creation.
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let mut command = portable_pty::CommandBuilder::new("cmd.exe");
        command.args(["/D", "/C", "exit", "7"]);
        assert!(pair
            .slave
            .spawn_command_in_job(command, file.as_handle())
            .is_err());
    }

    #[test]
    // This subprocess deliberately dies before it can wait; the parent verifies cleanup.
    #[allow(clippy::zombie_processes)]
    fn abrupt_exit_kills_children() {
        const MARKER: &str = "MONOCODE_JOB_TEST_CHILD";
        const PID_FILE: &str = "MONOCODE_JOB_TEST_DESCENDANT_FILE";
        if std::env::var(MARKER).as_deref() == Ok("terminal") {
            // This is the terminal's startup code. Launch a detached child
            // immediately, before the supervisor can do any post-spawn work.
            let mut command = Command::new("ping.exe");
            crate::hide_window_console(&mut command);
            let mut child = command
                .args(["-n", "60", "127.0.0.1"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            std::fs::write(std::env::var_os(PID_FILE).unwrap(), child.id().to_string()).unwrap();
            let _ = child.wait();
            return;
        }
        if std::env::var_os(MARKER).is_some() {
            initialize().unwrap();
            let mut command = Command::new("ping.exe");
            crate::hide_window_console(&mut command);
            command
                .args(["-n", "60", "127.0.0.1"])
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let child = spawn_managed(&mut command).unwrap();
            let pair = portable_pty::native_pty_system()
                .openpty(portable_pty::PtySize::default())
                .unwrap();
            let pid_file = std::env::temp_dir().join(format!(
                "monocode-pty-descendant-{}.pid",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&pid_file);
            let mut terminal = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
            terminal.args([
                "--exact",
                "windows::tests::abrupt_exit_kills_children",
                "--nocapture",
            ]);
            terminal.env(MARKER, "terminal");
            terminal.env(PID_FILE, &pid_file);
            // Act as the missing terminal UI: answer ConPTY's cursor query and
            // drain output so startup and shutdown cannot block on these pipes.
            let mut reader = pair.master.try_clone_reader().unwrap();
            let mut writer = pair.master.take_writer().unwrap();
            let terminal_output = Arc::new(Mutex::new(Vec::new()));
            let captured = terminal_output.clone();
            std::thread::spawn(move || {
                let mut buf = [0_u8; 1024];
                let query = b"\x1b[6n";
                let mut matched = 0;
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut output = captured.lock().unwrap();
                    if output.len() < 16 * 1024 {
                        output.extend_from_slice(&buf[..n]);
                    }
                    drop(output);
                    for &byte in &buf[..n] {
                        matched = if byte == query[matched] {
                            matched + 1
                        } else {
                            usize::from(byte == query[0])
                        };
                        if matched == query.len() {
                            let _ = writer.write_all(b"\x1b[1;1R");
                            matched = 0;
                        }
                    }
                }
            });
            let terminal = spawn_pty(pair.slave.as_ref(), terminal).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let descendant_pid: u32 = loop {
                if let Some(pid) = std::fs::read_to_string(&pid_file)
                    .ok()
                    .and_then(|text| text.parse().ok())
                {
                    break pid;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "terminal startup timed out; output: {:?}",
                    String::from_utf8_lossy(&terminal_output.lock().unwrap())
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            std::fs::remove_file(pid_file).unwrap();
            unsafe {
                // Check our specific job, since CI may already have its own job.
                let raw = OpenProcess(PROCESS_QUERY_INFORMATION, 0, descendant_pid);
                assert!(!raw.is_null());
                let descendant = OwnedHandle::from_raw_handle(raw);
                let mut in_job = 0;
                assert_ne!(
                    windows_sys::Win32::System::JobObjects::IsProcessInJob(
                        descendant.as_raw_handle(),
                        managed_job().unwrap().as_raw_handle(),
                        &mut in_job,
                    ),
                    0
                );
                assert_ne!(in_job, 0, "startup descendant escaped the managed job");
            }
            crate::hide_window_console(&mut command);
            let external = command.spawn().unwrap();
            println!("CHILD_PID={}", child.id());
            println!("TERMINAL_PID={}", terminal.process_id().unwrap());
            println!("DESCENDANT_PID={descendant_pid}");
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
        for prefix in ["CHILD_PID=", "TERMINAL_PID=", "DESCENDANT_PID="] {
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
