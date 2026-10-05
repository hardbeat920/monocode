//! A host-owned pipe stops Unix provider groups even after a hard host exit.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::Command;

pub(crate) struct ProviderGuard {
    _writer: OwnedFd,
    #[cfg(test)]
    guardian: libc::pid_t,
}

impl ProviderGuard {
    pub(crate) fn new(command: &mut Command) -> io::Result<Self> {
        let (read, write) = pipe_descriptors()?;
        // Keep the pipe outside stdin/stdout/stderr and close it on exec.
        let reader = duplicate(&read)?;
        let writer = duplicate(&write)?;
        drop(read);
        drop(write);
        let mut limit = unsafe { std::mem::zeroed::<libc::rlimit>() };
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let max_fd = limit.rlim_cur.min(i32::MAX as libc::rlim_t) as i32;
        let guardian = unsafe { libc::fork() };
        if guardian < 0 {
            return Err(io::Error::last_os_error());
        }
        if guardian == 0 {
            // The forked child never enters Rust runtime cleanup or allocates.
            unsafe { watch_pipe(reader.as_raw_fd(), max_fd) }
        }
        drop(reader);
        if let Err(error) = std::thread::Builder::new()
            .name("provider-guard-wait".into())
            .spawn(move || reap(guardian))
        {
            drop(writer);
            reap(guardian);
            return Err(error);
        }
        let fd = writer.as_raw_fd();
        unsafe {
            command.pre_exec(move || {
                // Command already assigned the provider's process group.
                // Notify before exec so a host death during startup is covered.
                let pid = libc::getpid();
                let bytes = std::mem::size_of_val(&pid);
                let mut written = 0;
                while written < bytes {
                    let count = libc::write(
                        fd,
                        (&pid as *const libc::pid_t)
                            .cast::<u8>()
                            .add(written)
                            .cast(),
                        bytes - written,
                    );
                    if count < 0 {
                        let error = io::Error::last_os_error();
                        if error.raw_os_error() == Some(libc::EINTR) {
                            continue;
                        }
                        return Err(error);
                    }
                    if count == 0 {
                        return Err(io::Error::from_raw_os_error(libc::EPIPE));
                    }
                    written += count as usize;
                }
                Ok(())
            });
        }
        // OwnedFd closes the host writer after the child is unregistered.
        Ok(Self {
            _writer: writer,
            #[cfg(test)]
            guardian,
        })
    }
}

#[cfg(target_os = "linux")]
fn pipe_descriptors() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut pipe = [-1; 2];
    if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(pipe[0]), OwnedFd::from_raw_fd(pipe[1])) })
}

#[cfg(not(target_os = "linux"))]
fn pipe_descriptors() -> io::Result<(OwnedFd, OwnedFd)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    // Darwin has no pipe2. Opening an unlinked FIFO with O_CLOEXEC prevents
    // simultaneous execs from inheriting even the temporary pipe descriptors.
    let path = std::env::temp_dir().join(format!(
        "monocode-provider-guard-{}-{}.fifo",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let name = CString::new(path.as_os_str().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if unsafe { libc::mkfifo(name.as_ptr(), 0o600) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let endpoints = (|| {
        let read = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if read < 0 {
            return Err(io::Error::last_os_error());
        }
        let read = unsafe { OwnedFd::from_raw_fd(read) };
        let write = unsafe { libc::open(name.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
        if write < 0 {
            return Err(io::Error::last_os_error());
        }
        let write = unsafe { OwnedFd::from_raw_fd(write) };
        if unsafe { libc::fcntl(read.as_raw_fd(), libc::F_SETFL, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((read, write))
    })();
    if unsafe { libc::unlink(name.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    endpoints
}

fn duplicate(fd: &OwnedFd) -> io::Result<OwnedFd> {
    let copy = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if copy < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(copy) })
    }
}

fn reap(pid: libc::pid_t) {
    loop {
        if unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) } >= 0 {
            return;
        }
        if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

unsafe fn watch_pipe(reader: i32, max_fd: i32) -> ! {
    unsafe {
        if libc::setpgid(0, 0) != 0 || libc::dup2(reader, 0) < 0 {
            libc::_exit(1);
        }
        close_other_descriptors(max_fd);
        let mut group: libc::pid_t = 0;
        loop {
            let mut next: libc::pid_t = 0;
            let bytes = std::mem::size_of_val(&next);
            let mut received = 0;
            while received < bytes {
                let count = libc::read(
                    0,
                    (&mut next as *mut libc::pid_t)
                        .cast::<u8>()
                        .add(received)
                        .cast(),
                    bytes - received,
                );
                if count < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                if count <= 0 {
                    stop_group(group);
                }
                received += count as usize;
            }
            // ETXTBSY retries can report a replacement before the final exec.
            group = next;
        }
    }
}

unsafe fn close_other_descriptors(max_fd: i32) {
    #[cfg(target_os = "linux")]
    if unsafe { libc::syscall(libc::SYS_close_range, 1u32, u32::MAX, 0u32) } == 0 {
        return;
    }
    for fd in 1..max_fd {
        unsafe { libc::close(fd) };
    }
}

unsafe fn stop_group(group: libc::pid_t) -> ! {
    unsafe {
        if group <= 1 || libc::kill(-group, libc::SIGTERM) != 0 {
            libc::_exit(0);
        }
        let mut remaining = libc::timespec {
            tv_sec: 1,
            tv_nsec: 0,
        };
        loop {
            let mut interrupted = std::mem::zeroed::<libc::timespec>();
            if libc::nanosleep(&remaining, &mut interrupted) == 0 {
                break;
            }
            remaining = interrupted;
        }
        libc::kill(-group, libc::SIGKILL);
        libc::_exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_spawn_releases_the_native_guard() {
        let missing = format!("/monocode-missing-provider-{}", std::process::id());
        assert!(!std::path::Path::new(&missing).exists());
        let mut command = Command::new(missing);
        command.process_group(0);
        let guard = ProviderGuard::new(&mut command).unwrap();
        let guardian = guard.guardian;
        assert!(command.spawn().is_err());
        drop(guard);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while unsafe { libc::kill(guardian, 0) } == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "a failed spawn left its guardian alive"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
