use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct DesktopChild(Option<Child>);

impl DesktopChild {
    fn child(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }

    fn output(mut self) -> Output {
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}

impl Drop for DesktopChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn desktop_answers_an_authenticated_ssh_prompt_without_starting_gpui() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let prompt = "Are you sure you want to continue connecting (yes/no/[fingerprint])?";
    let mut child = DesktopChild(Some(
        Command::new(env!("CARGO_BIN_EXE_monocode-app"))
            .arg(prompt)
            .env(
                "MONOCODE_SSH_ASKPASS_ADDRESS",
                listener.local_addr().unwrap().to_string(),
            )
            .env("MONOCODE_SSH_ASKPASS_SECRET", &nonce)
            .env("SSH_ASKPASS_PROMPT", "confirm")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(status) = child.child().try_wait().unwrap() {
                    let output = child.output();
                    panic!(
                        "desktop exited before handling SSH askpass: {status}; {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                if Instant::now() >= deadline {
                    panic!("desktop did not handle SSH askpass within five seconds");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept SSH prompt: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut line = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    let request: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(request["secret"] == nonce);
    assert_eq!(request["prompt"], prompt);
    assert_eq!(request["confirm"], true);
    stream.write_all(b"\"yes\"\n").unwrap();
    while child.child().try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            panic!("desktop did not exit after answering SSH askpass");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.output();
    assert!(output.status.success(), "{:?}", output.status);
    assert_eq!(output.stdout, b"yes\n");
    assert!(output.stderr.is_empty());
}
