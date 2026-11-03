use crate::mobile_access::{self, MobileHost};
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
pub fn restart(host: &MobileHost) {
    if !mobile_access::router_claim(host) {
        return;
    }
    let service = host.clone();
    tauri::async_runtime::spawn(async move {
        let mut mapping: Option<Mapping> = None;
        let mut attempted = false;
        let mut renewed = Instant::now();
        loop {
            if !mobile_access::router_config(&service) {
                if let Some(old) = mapping.take() {
                    let _ = tauri::async_runtime::spawn_blocking(move || drop(old)).await;
                }
                attempted = false;
                mobile_access::router_result(&service, None, "Off".into());
            } else if !attempted {
                attempted = true;
                mobile_access::router_result(&service, None, "Asking the router…".into());
                match tauri::async_runtime::spawn_blocking(map).await {
                    Ok(Ok(value)) => {
                        // Recheck after blocking router discovery; disabling never
                        // leaves a newly discovered address advertised.
                        if mobile_access::router_config(&service) {
                            mobile_access::router_result(
                                &service,
                                Some(value.address()),
                                "Open".into(),
                            );
                            mapping = Some(value);
                            renewed = Instant::now();
                        } else {
                            let _ = tauri::async_runtime::spawn_blocking(move || drop(value)).await;
                        }
                    }
                    Ok(Err(error)) => mobile_access::router_result(&service, None, error),
                    Err(_) => {
                        mobile_access::router_result(&service, None, "Router setup failed".into())
                    }
                }
            } else if renewed.elapsed() >= Duration::from_secs(1200) {
                if let Some(value) = mapping.take() {
                    match tauri::async_runtime::spawn_blocking(move || {
                        let result = value.renew();
                        (value, result)
                    })
                    .await
                    {
                        Ok((value, Ok(()))) => {
                            mapping = Some(value);
                            renewed = Instant::now();
                        }
                        Ok((value, Err(_))) => {
                            let _ = tauri::async_runtime::spawn_blocking(move || drop(value)).await;
                            mobile_access::router_result(
                                &service,
                                None,
                                "Router mapping expired. Turn router access off and on to retry."
                                    .into(),
                            );
                        }
                        Err(_) => mobile_access::router_result(
                            &service,
                            None,
                            "Router renewal failed".into(),
                        ),
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}
