//! Whether the internet answers: TCP handshakes, all at once, with hosts run
//! by different companies over IPv4 and IPv6, plus the provider by name. One
//! answer is enough, so a network that blocks a host or two still reads as
//! online, and the whole check is bounded even when DNS hangs.
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const ADDRESSES: [&str; 6] = [
    "1.1.1.1:443",
    "8.8.8.8:443",
    "9.9.9.9:443",
    "208.67.222.222:443",
    "[2606:4700:4700::1111]:443",
    "[2001:4860:4860::8888]:443",
];
const NAMED: (&str, u16) = ("api.anthropic.com", 443);

type Check = Box<dyn FnOnce() -> bool + Send>;

pub fn online(timeout: Duration) -> bool {
    let reach = move |addr: SocketAddr| TcpStream::connect_timeout(&addr, timeout).is_ok();
    let mut checks: Vec<Check> = ADDRESSES
        .iter()
        .filter_map(|a| a.parse::<SocketAddr>().ok())
        .map(|addr| Box::new(move || reach(addr)) as Check)
        .collect();
    checks.push(Box::new(move || {
        NAMED
            .to_socket_addrs()
            .into_iter()
            .flatten()
            .take(2)
            .any(reach)
    }));
    // DNS gets as long again as a handshake.
    any_answers(checks, timeout * 2)
}

/// Runs every check on its own thread; true as soon as one passes, false
/// once all fail or `limit` passes. A check that hangs is left behind.
pub(crate) fn any_answers(checks: Vec<Check>, limit: Duration) -> bool {
    let (tx, rx) = mpsc::channel();
    let count = checks.len();
    for check in checks {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check());
        });
    }
    drop(tx);
    let deadline = Instant::now() + limit;
    for _ in 0..count {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hang() -> Check {
        Box::new(|| {
            std::thread::sleep(Duration::from_secs(10));
            true
        })
    }

    #[test]
    fn a_hung_probe_does_not_stall_the_answer() {
        // The old probe tried hosts one after another and then an untimed
        // DNS lookup; one stuck step held the whole check.
        let start = Instant::now();
        assert!(any_answers(
            vec![hang(), Box::new(|| true)],
            Duration::from_secs(5)
        ));
        assert!(!any_answers(vec![hang()], Duration::from_millis(200)));
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn one_answer_is_enough() {
        let blocked = || Box::new(|| false) as Check;
        assert!(any_answers(
            vec![blocked(), blocked(), Box::new(|| true)],
            Duration::from_secs(1)
        ));
        assert!(!any_answers(
            vec![blocked(), blocked()],
            Duration::from_secs(1)
        ));
        assert!(!any_answers(Vec::new(), Duration::from_secs(1)));
    }
}
