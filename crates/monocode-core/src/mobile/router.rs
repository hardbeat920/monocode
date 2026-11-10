//! Automatic NAT-PMP/UPnP UDP mapping so a phone can reach the desktop away
//! from its network.
use super::host::MobileHost;
use monocode_mobile_link::{Address, PORT};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

enum Mapping {
    NatPmp {
        port: u16,
        host: Ipv4Addr,
    },
    Upnp {
        gateway: Box<igd_next::Gateway>,
        port: u16,
        local: SocketAddr,
        host: IpAddr,
    },
}
impl Mapping {
    fn address(&self) -> Address {
        match self {
            Self::NatPmp { port, host } => Address {
                host: host.to_string(),
                port: *port,
            },
            Self::Upnp { port, host, .. } => Address {
                host: host.to_string(),
                port: *port,
            },
        }
    }
    fn renew(&self) -> Result<(), String> {
        match self {
            Self::NatPmp { port, .. } => nat_mapping(*port, 3600).map(|_| ()),
            Self::Upnp {
                gateway,
                port,
                local,
                ..
            } => gateway
                .add_port(
                    igd_next::PortMappingProtocol::UDP,
                    *port,
                    *local,
                    3600,
                    "MonoCode mobile",
                )
                .map_err(|e| e.to_string()),
        }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        match self {
            Self::NatPmp { port, .. } => {
                let _ = nat_mapping(*port, 0);
            }
            Self::Upnp { gateway, port, .. } => {
                let _ = gateway.remove_port(igd_next::PortMappingProtocol::UDP, *port);
            }
        }
    }
}
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let octets = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_multicast()
                && !(octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        IpAddr::V6(v) => {
            !v.is_loopback()
                && !v.is_unspecified()
                && !v.is_unicast_link_local()
                && !v.is_unique_local()
        }
    }
}
fn response(client: &mut natpmp::Natpmp) -> Result<natpmp::Response, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(response) = client.read_response_or_retry() {
            return Ok(response);
        }
        if Instant::now() >= deadline {
            return Err("Router did not answer NAT-PMP".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn nat_mapping(port: u16, lifetime: u32) -> Result<(Ipv4Addr, u16), String> {
    let mut client = natpmp::Natpmp::new().map_err(|e| e.to_string())?;
    client
        .send_public_address_request()
        .map_err(|e| e.to_string())?;
    let natpmp::Response::Gateway(gateway) = response(&mut client)? else {
        return Err("Invalid NAT-PMP response".into());
    };
    let host = *gateway.public_address();
    if lifetime != 0 && !public(IpAddr::V4(host)) {
        return Err("Router is behind another NAT. Add a reachable remote address.".into());
    }
    client
        .send_port_mapping_request(natpmp::Protocol::UDP, PORT, port, lifetime)
        .map_err(|e| e.to_string())?;
    let natpmp::Response::UDP(mapping) = response(&mut client)? else {
        return Err("Invalid NAT-PMP mapping".into());
    };
    Ok((host, mapping.public_port()))
}
fn map() -> Result<Mapping, String> {
    if let Ok((host, port)) = nat_mapping(PORT, 3600) {
        return Ok(Mapping::NatPmp { port, host });
    }
    let mut search = igd_next::SearchOptions::default();
    search.timeout = Some(Duration::from_secs(3));
    let gateway = igd_next::search_gateway(search).map_err(|_| "Router could not open a port. Add a reachable remote address or forward the UDP port manually.".to_string())?;
    let host = gateway.get_external_ip().map_err(|e| e.to_string())?;
    if !public(host) {
        return Err("Router is behind another NAT. Add a reachable remote address.".into());
    }
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket.connect(gateway.addr).map_err(|e| e.to_string())?;
    let local = SocketAddr::new(socket.local_addr().map_err(|e| e.to_string())?.ip(), PORT);
    let port = gateway
        .add_any_port(
            igd_next::PortMappingProtocol::UDP,
            local,
            3600,
            "MonoCode mobile",
        )
        .map_err(|e| e.to_string())?;
    Ok(Mapping::Upnp {
        gateway: Box::new(gateway),
        port,
        local,
        host,
    })
}
// A single manager owns the lease. It finishes removing an old mapping before
// making another, including when configuration changes during discovery.
// Router discovery and renewal block, so the manager runs on its own thread.
pub(super) fn restart(host: &MobileHost) {
    if !host.router_claim() {
        return;
    }
    let service = host.clone();
    let spawned = std::thread::Builder::new()
        .name("monocode-mobile-router".into())
        .spawn(move || {
            let mut mapping: Option<Mapping> = None;
            let mut attempted = false;
            let mut renewed = Instant::now();
            loop {
                if !service.router_config() {
                    drop(mapping.take());
                    attempted = false;
                    service.router_result(None, "Off".into());
                } else if !attempted {
                    attempted = true;
                    service.router_result(None, "Asking the router…".into());
                    match map() {
                        // Recheck after blocking router discovery; disabling never
                        // leaves a newly discovered address advertised.
                        Ok(value) if service.router_config() => {
                            service.router_result(Some(value.address()), "Open".into());
                            mapping = Some(value);
                            renewed = Instant::now();
                        }
                        Ok(value) => drop(value),
                        Err(error) => service.router_result(None, error),
                    }
                } else if renewed.elapsed() >= Duration::from_secs(1200) {
                    if let Some(value) = mapping.take() {
                        match value.renew() {
                            Ok(()) => {
                                mapping = Some(value);
                                renewed = Instant::now();
                            }
                            Err(_) => {
                                drop(value);
                                service.router_result(
                                    None,
                                    "Router mapping expired. Turn router access off and on to retry."
                                        .into(),
                                );
                            }
                        }
                    }
                }
                if service.is_shut_down() {
                    drop(mapping.take());
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    if spawned.is_err() {
        host.router_result(None, "Router setup failed".into());
    }
}
