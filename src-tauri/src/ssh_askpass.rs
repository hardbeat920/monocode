//! OpenSSH calls the desktop executable in a small, non-GUI askpass mode.
//! Answers travel over a nonce-authenticated loopback socket, never argv/files.
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

const ADDRESS: &str = "MONOCODE_SSH_ASKPASS_ADDRESS";
const SECRET: &str = "MONOCODE_SSH_ASKPASS_SECRET";

#[derive(Serialize, Deserialize)]
struct Request {
    secret: String,
    prompt: String,
    confirm: bool,
}

fn confirmation_prompt(prompt: &str, hint: &str) -> bool {
    hint == "confirm" || prompt.contains("(yes/no/[fingerprint])") || prompt.contains("(yes/no)")
}

pub fn maybe_run() -> Option<i32> {
    let address = std::env::var(ADDRESS).ok()?;
    Some(
        (|| -> Result<i32, Box<dyn std::error::Error>> {
            let address: SocketAddr = address.parse()?;
            if !address.ip().is_loopback() {
                return Ok(1);
            }
            let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
            stream.set_read_timeout(Some(Duration::from_secs(130)))?;
            stream.set_write_timeout(Some(Duration::from_secs(3)))?;
            let prompt = std::env::args()
                .nth(1)
                .unwrap_or_else(|| "SSH authentication".into());
            let confirm = confirmation_prompt(
                &prompt,
                &std::env::var("SSH_ASKPASS_PROMPT").unwrap_or_default(),
            );
            let request = Request {
                secret: std::env::var(SECRET)?,
                prompt,
                confirm,
            };
            writeln!(stream, "{}", serde_json::to_string(&request)?)?;
            let line = read_line(&mut stream)?;
            let answer: Option<String> = serde_json::from_str(&line)?;
            match answer {
                Some(answer) => {
                    println!("{answer}");
                    Ok(if confirm && answer != "yes" { 1 } else { 0 })
                }
                None => Ok(1),
            }
        })()
        .unwrap_or(1),
    )
}

fn read_line(stream: &mut TcpStream) -> Result<String, String> {
    use std::io::Read;
    let mut line = String::new();
    BufReader::new(stream)
        .take(16 * 1024)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if !line.ends_with('\n') {
        return Err("SSH prompt exceeded its size limit".into());
    }
    Ok(line)
}

pub(crate) struct Askpass {
    address: SocketAddr,
    secret: String,
    stopped: Arc<AtomicBool>,
}
impl Askpass {
    pub fn start(
        handler: impl Fn(String, bool) -> Option<String> + Send + 'static,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let stopped = Arc::new(AtomicBool::new(false));
        let shutdown = stopped.clone();
        let expected = secret.clone();
        std::thread::spawn(move || {
            while !shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
                        let request = read_line(&mut stream)
                            .ok()
                            .and_then(|line| serde_json::from_str::<Request>(&line).ok());
                        if let Some(request) = request.filter(|r| r.secret == expected) {
                            let answer = handler(request.prompt, request.confirm);
                            let _ = writeln!(
                                stream,
                                "{}",
                                serde_json::to_string(&answer).unwrap_or_else(|_| "null".into())
                            );
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address,
            secret,
            stopped,
        })
    }
    pub fn configure(&self, command: &mut Command) -> Result<(), String> {
        command
            .env(
                "SSH_ASKPASS",
                std::env::current_exe().map_err(|e| e.to_string())?,
            )
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "monocode:0")
            .env(ADDRESS, self.address.to_string())
            .env(SECRET, &self.secret);
        Ok(())
    }
}
impl Drop for Askpass {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_openssh_host_trust_without_a_prompt_hint() {
        assert!(confirmation_prompt(
            "Are you sure you want to continue connecting (yes/no/[fingerprint])?",
            ""
        ));
        assert!(confirmation_prompt("Allow access?", "confirm"));
        assert!(!confirmation_prompt("Enter passphrase for key:", ""));
        assert!(!confirmation_prompt("user@host's password:", ""));
    }
    #[test]
    fn prompt_bridge_rejects_unknown_clients_and_does_not_put_answers_in_files() {
        let bridge = Askpass::start(|prompt, confirm| {
            assert_eq!(prompt, "Passphrase?");
            assert!(!confirm);
            Some("test secret".into())
        })
        .unwrap();
        let mut stream = TcpStream::connect(bridge.address).unwrap();
        writeln!(
            stream,
            "{}",
            serde_json::to_string(&Request {
                secret: "wrong".into(),
                prompt: "ignored".into(),
                confirm: false
            })
            .unwrap()
        )
        .unwrap();
        assert!(read_line(&mut stream).is_err());
        let mut stream = TcpStream::connect(bridge.address).unwrap();
        writeln!(
            stream,
            "{}",
            serde_json::to_string(&Request {
                secret: bridge.secret.clone(),
                prompt: "Passphrase?".into(),
                confirm: false
            })
            .unwrap()
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<String>(&read_line(&mut stream).unwrap()).unwrap(),
            "test secret"
        );
    }
}
