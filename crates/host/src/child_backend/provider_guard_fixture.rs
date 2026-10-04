use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[cfg(unix)]
unsafe extern "C" {
    fn getpgrp() -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn signal(signal: i32, handler: usize) -> usize;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn GetExitCodeProcess(process: *mut std::ffi::c_void, code: *mut u32) -> i32;
    fn TerminateProcess(process: *mut std::ffi::c_void, code: u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

fn identity(root: &Path, role: &str) {
    #[cfg(unix)]
    let group = unsafe { getpgrp() } as u32;
    #[cfg(windows)]
    let group = 0;
    let staging = root.join(format!("{role}.tmp"));
    std::fs::write(&staging, format!("{} {group}\n", std::process::id())).unwrap();
    std::fs::rename(staging, root.join(format!("{role}.pid"))).unwrap();
}

fn alive(pid: u32, group: bool) -> bool {
    #[cfg(unix)]
    {
        unsafe { kill(if group { -(pid as i32) } else { pid as i32 }, 0) == 0 }
    }
    #[cfg(windows)]
    {
        assert!(!group);
        let process = unsafe { OpenProcess(0x1000, 0, pid) };
        if process.is_null() {
            return false;
        }
        let mut code = 0;
        let running = unsafe { GetExitCodeProcess(process, &mut code) != 0 && code == 259 };
        unsafe { CloseHandle(process) };
        running
    }
}

fn stop(pid: u32) {
    #[cfg(unix)]
    unsafe {
        kill(pid as i32, 9);
    }
    #[cfg(windows)]
    unsafe {
        let process = OpenProcess(0x0001, 0, pid);
        if !process.is_null() {
            TerminateProcess(process, 1);
            CloseHandle(process);
        }
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--version"] {
        println!("codex 1.0.0 fixture");
        return;
    }
    let role = args.first().unwrap().as_str();
    match role {
        "alive" | "group-alive" => {
            std::process::exit(if alive(args[1].parse().unwrap(), role == "group-alive") {
                0
            } else {
                1
            });
        }
        "stop" => {
            stop(args[1].parse().unwrap());
            return;
        }
        _ => {}
    }
    let root = Path::new(&args[1]);
    if role == "provider" {
        identity(root, role);
        let _descendant = Command::new(std::env::current_exe().unwrap())
            .arg("descendant")
            .arg(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        for line in std::io::stdin().lock().lines() {
            let line = line.unwrap();
            if line == "quit" {
                return;
            }
            if line == "ping" {
                println!("pong");
                std::io::stdout().flush().unwrap();
            }
        }
    } else {
        #[cfg(unix)]
        unsafe {
            signal(15, 1);
        }
        identity(root, role);
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
