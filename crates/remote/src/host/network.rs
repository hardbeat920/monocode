//! Port of host/network.ts: network access settings, the addresses a host
//! advertises, and the pairing link codec.

use std::net::IpAddr;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::exec::{ExecOptions, exec};

/// Saved in `<data dir>/network.json`. With network access off, the host
/// listens only on loopback and is reached through an SSH forward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSettings {
    pub enabled: bool,
    pub bind: String,
}

pub const DEFAULT_BIND: &str = "0.0.0.0";

pub fn read_network_settings(directory: &Path) -> NetworkSettings {
    let fallback = NetworkSettings {
        enabled: false,
        bind: DEFAULT_BIND.into(),
    };
    let Ok(text) = std::fs::read_to_string(directory.join("network.json")) else {
        return fallback;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return fallback;
    };
    NetworkSettings {
        enabled: value.get("enabled") == Some(&Value::Bool(true)),
        bind: value
            .get("bind")
            .and_then(Value::as_str)
            .filter(|bind| !bind.is_empty())
            .unwrap_or(DEFAULT_BIND)
            .into(),
    }
}

pub fn write_network_settings(directory: &Path, settings: &NetworkSettings) -> Result<(), String> {
    let path = directory.join("network.json");
    let temporary = directory.join("network.json.tmp");
    let text = serde_json::to_string(settings).map_err(|error| error.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(
        &mut options
            .open(&temporary)
            .map_err(|error| error.to_string())?,
        text.as_bytes(),
    )
    .map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

/// One address of a network interface, as `os.networkInterfaces()` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAddress {
    pub name: String,
    pub address: IpAddr,
    /// A loopback address.
    pub internal: bool,
}

/// This machine's interface addresses.
pub fn network_interfaces() -> Vec<InterfaceAddress> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|interface| InterfaceAddress {
            internal: interface.is_loopback(),
            address: interface.ip(),
            name: interface.name,
        })
        .collect()
}

// Container bridges, VM switches, and WSL's host-side adapter are not
// reachable from another computer.
fn is_virtual(name: &str) -> bool {
    static VIRTUAL: OnceLock<regex::Regex> = OnceLock::new();
    VIRTUAL
        .get_or_init(|| {
            regex::Regex::new(
                r"(?i)^(docker|br-|veth|virbr|vmnet|vboxnet|lxc|lxd|cni|flannel|podman|awdl|llw|bridge|vEthernet \(WSL)",
            )
            .expect("valid pattern")
        })
        .is_match(name)
}

fn private_range(address: &[u8; 4]) -> bool {
    address[0] == 10
        || (address[0] == 192 && address[1] == 168)
        || (address[0] == 172 && (16..=31).contains(&address[1]))
}

// Tailscale and other overlay networks use the carrier-grade NAT range.
fn overlay_range(address: &[u8; 4]) -> bool {
    address[0] == 100 && (64..=127).contains(&address[1])
}

/// Candidate `https://` addresses for this host, most likely to work first:
/// private LAN addresses, then overlay networks such as Tailscale, then the
/// rest. Desktops try each and keep the one that answers.
pub fn network_endpoints(
    port: u16,
    bind: &str,
    interfaces: &[InterfaceAddress],
    names: &[String],
) -> Vec<String> {
    let url = |host: &str| {
        if host.contains(':') {
            format!("https://[{host}]:{port}")
        } else {
            format!("https://{host}:{port}")
        }
    };
    if bind != "0.0.0.0" && bind != "::" {
        return vec![url(bind)];
    }
    let mut addresses: Vec<[u8; 4]> = Vec::new();
    for entry in interfaces {
        if is_virtual(&entry.name) || entry.internal {
            continue;
        }
        let IpAddr::V4(address) = entry.address else {
            continue;
        };
        let octets = address.octets();
        if octets[0] == 169 && octets[1] == 254 {
            continue;
        }
        if !addresses.contains(&octets) {
            addresses.push(octets);
        }
    }
    let rank = |address: &[u8; 4]| {
        if private_range(address) {
            0
        } else if overlay_range(address) {
            1
        } else {
            2
        }
    };
    addresses.sort_by_key(rank);
    addresses
        .iter()
        .map(|octets| std::net::Ipv4Addr::from(*octets).to_string())
        .chain(names.iter().cloned())
        .map(|host| url(&host))
        .collect()
}

/// This machine's Tailscale MagicDNS name, when Tailscale is running.
pub fn tailscale_name() -> Option<String> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "tailscale",
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        ]
    } else if cfg!(windows) {
        &[
            "tailscale.exe",
            "C:\\Program Files\\Tailscale\\tailscale.exe",
        ]
    } else {
        &["tailscale"]
    };
    for command in candidates {
        let Ok(output) = exec(
            command,
            &["status", "--json", "--peers=false"],
            ExecOptions {
                timeout: Duration::from_secs(3),
                max_buffer: 4 * 1024 * 1024,
                ..Default::default()
            },
        ) else {
            // Not installed, or not running.
            continue;
        };
        let Ok(status) = serde_json::from_str::<Value>(&output.stdout) else {
            continue;
        };
        let name = status
            .get("Self")
            .and_then(|own| own.get("DNSName"))
            .and_then(Value::as_str)
            .map(|name| name.strip_suffix('.').unwrap_or(name));
        return name
            .filter(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
            })
            .map(str::to_string);
    }
    None
}

/// What a pairing link carries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PairingOffer {
    pub name: String,
    pub environment_id: String,
    pub fingerprint: String,
    pub code: String,
    pub endpoints: Vec<String>,
}

/// A link the desktop accepts in Settings, Connections, Pair machine. The
/// code is single-use; the fingerprint pins the host's TLS certificate.
pub fn pairing_link(offer: &PairingOffer) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("v", "1")
        .append_pair("name", &offer.name)
        .append_pair("id", &offer.environment_id)
        .append_pair("fp", &offer.fingerprint)
        .append_pair("code", &offer.code);
    for endpoint in &offer.endpoints {
        query.append_pair("url", endpoint);
    }
    format!("monocode://pair?{}", query.finish())
}

pub fn parse_pairing_link(link: &str) -> Result<PairingOffer, String> {
    let url =
        url::Url::parse(monocode_core::js::trim(link)).map_err(|_| "Invalid URL".to_string())?;
    let first = |key: &str| {
        url.query_pairs()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.into_owned())
    };
    if url.scheme() != "monocode"
        || url.host_str() != Some("pair")
        || first("v").as_deref() != Some("1")
    {
        return Err("Not a MonoCode pairing link".into());
    }
    Ok(PairingOffer {
        name: first("name").unwrap_or_default(),
        environment_id: first("id").unwrap_or_default(),
        fingerprint: first("fp").unwrap_or_default(),
        code: first("code").unwrap_or_default(),
        endpoints: url
            .query_pairs()
            .filter(|(name, _)| name == "url")
            .map(|(_, value)| value.into_owned())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, address: &str, internal: bool) -> InterfaceAddress {
        InterfaceAddress {
            name: name.into(),
            address: address.parse().unwrap(),
            internal,
        }
    }

    #[test]
    fn ranks_lan_addresses_before_overlay_networks_and_skips_virtual_adapters() {
        assert_eq!(
            network_endpoints(
                3774,
                "0.0.0.0",
                &[
                    entry("lo0", "127.0.0.1", true),
                    entry("tailscale0", "100.64.0.9", false),
                    entry("en0", "192.168.1.20", false),
                    entry("en0", "169.254.3.3", false),
                    entry("en0", "fe80::1", false),
                    entry("docker0", "172.17.0.1", false),
                    entry("vEthernet (WSL)", "172.28.0.1", false),
                    entry("eth1", "203.0.113.8", false),
                ],
                &["box.tailnet.ts.net".into()],
            ),
            [
                "https://192.168.1.20:3774",
                "https://100.64.0.9:3774",
                "https://203.0.113.8:3774",
                "https://box.tailnet.ts.net:3774",
            ]
        );
        assert_eq!(
            network_endpoints(3774, "10.0.0.2", &network_interfaces(), &[]),
            ["https://10.0.0.2:3774"]
        );
        assert_eq!(
            network_endpoints(3774, "fd7a::1", &[], &[]),
            ["https://[fd7a::1]:3774"]
        );
    }

    #[test]
    fn round_trips_a_pairing_link() {
        let offer = PairingOffer {
            name: "Studio & lab".into(),
            environment_id: "env".into(),
            fingerprint: "f".repeat(43),
            code: "c".repeat(43),
            endpoints: vec![
                "https://10.0.0.2:3774".into(),
                "https://box.ts.net:3774".into(),
            ],
        };
        let link = pairing_link(&offer);
        assert!(link.starts_with("monocode://pair?v=1&"));
        // URLSearchParams encoding, as the TypeScript host printed it.
        assert!(link.contains("name=Studio+%26+lab&id=env&"), "{link}");
        assert!(
            link.ends_with(
                "&url=https%3A%2F%2F10.0.0.2%3A3774&url=https%3A%2F%2Fbox.ts.net%3A3774"
            )
        );
        assert_eq!(parse_pairing_link(&format!("  {link}\n")).unwrap(), offer);
        assert!(parse_pairing_link("https://example.com/pair?v=1").is_err());
        assert!(parse_pairing_link("monocode://pair?v=2").is_err());
    }

    #[test]
    fn reads_missing_or_broken_settings_as_loopback_only() {
        let directory = crate::host::store::tests::temporary("monocode-network-");
        let off = NetworkSettings {
            enabled: false,
            bind: DEFAULT_BIND.into(),
        };
        assert_eq!(read_network_settings(directory.path()), off);
        std::fs::write(directory.path().join("network.json"), "{oops").unwrap();
        assert_eq!(read_network_settings(directory.path()), off);
        std::fs::write(
            directory.path().join("network.json"),
            r#"{"enabled":"yes","bind":""}"#,
        )
        .unwrap();
        assert_eq!(read_network_settings(directory.path()), off);
        let on = NetworkSettings {
            enabled: true,
            bind: "127.0.0.1".into(),
        };
        write_network_settings(directory.path(), &on).unwrap();
        assert_eq!(read_network_settings(directory.path()), on);
        assert_eq!(
            std::fs::read_to_string(directory.path().join("network.json")).unwrap(),
            r#"{"enabled":true,"bind":"127.0.0.1"}"#
        );
    }
}
