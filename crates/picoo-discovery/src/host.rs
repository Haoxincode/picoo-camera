//! LAN advertise / manual-connect host selection — REQ-PICOO-DISCOVERY-001 / 006.
//!
//! Receiver binds `0.0.0.0` but must advertise a reachable unicast IPv4 to phones
//! on the same LAN (never loopback / unspecified / link-local).
//!
//! Virtual adapters (VPN, Docker Desktop, WSL/Hyper-V, VMware) often expose RFC1918
//! addresses that phones cannot reach; interface-aware selection excludes those adapters
//! and prefers Wi‑Fi / Ethernet among the remaining physical-LAN candidates.

use std::net::{IpAddr, Ipv4Addr};

/// Default QUIC UDP port (aligned with WiX FirewallException).
pub const DEFAULT_QUIC_PORT: u16 = 4433;

/// Choose the best IPv4 to put in mDNS A records and the manual-connect endpoint.
///
/// Preference when no interface names are available: RFC1918 private → other global
/// unicast → none. Skips loopback, unspecified, and link-local (169.254/16).
pub fn select_advertise_ipv4(candidates: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    let usable: Vec<Ipv4Addr> = candidates
        .iter()
        .copied()
        .filter(|ip| is_advertise_candidate(*ip))
        .collect();
    let mut scored: Vec<(i32, Ipv4Addr)> = usable
        .iter()
        .copied()
        .map(|ip| (score_ip_only(ip), ip))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.octets().cmp(&b.1.octets())));
    scored.first().map(|(_, ip)| *ip)
}

/// Interface-aware selection — preferred for desktop receivers on Windows with Docker/WSL.
pub fn select_advertise_ipv4_with_interfaces(ifaces: &[(String, Ipv4Addr)]) -> Option<Ipv4Addr> {
    let mut scored: Vec<(i32, Ipv4Addr)> = ifaces
        .iter()
        .filter(|(name, ip)| is_advertise_candidate(*ip) && !is_virtual_or_tunnel_interface(name))
        .map(|(name, ip)| (score_advertise_candidate(name, *ip), *ip))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.octets().cmp(&b.1.octets())));
    scored.first().map(|(_, ip)| *ip)
}

/// Score a candidate using interface name heuristics and IPv4 prefix hints.
pub fn score_advertise_candidate(interface_name: &str, ip: Ipv4Addr) -> i32 {
    let lower = interface_name.to_lowercase();
    let mut score = score_ip_only(ip);

    if lower.contains("wi-fi")
        || lower.contains("wifi")
        || lower.contains("wlan")
        || lower.contains("wireless")
    {
        score += 200;
    } else if lower.contains("ethernet") || lower.contains("eth") || lower.contains("en0") {
        score += 150;
    }

    if is_virtual_or_tunnel_interface(&lower) {
        score -= 500;
    }

    score
}

fn is_virtual_or_tunnel_interface(interface_name: &str) -> bool {
    let lower = interface_name.to_lowercase();
    [
        "docker",
        "wsl",
        "hyper-v",
        "vethernet",
        "virtual",
        "vmware",
        "vbox",
        "npcap",
        "loopback",
        "tunnel",
        "tailscale",
        "zerotier",
        "wireguard",
        "openvpn",
        "vpn",
        "utun",
        "ppp",
        "tap",
        "tun",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn score_ip_only(ip: Ipv4Addr) -> i32 {
    let o = ip.octets();
    if o[0] == 192 && o[1] == 168 {
        return 80;
    }
    if o[0] == 10 {
        return 60;
    }
    // 172.16/12 is RFC1918 but frequently Docker Desktop / Hyper-V internal switches.
    if o[0] == 172 && (16..=31).contains(&o[1]) {
        return 10;
    }
    20
}

fn is_advertise_candidate(ip: Ipv4Addr) -> bool {
    !ip.is_unspecified() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_multicast()
}

/// Enumerate local interface IPv4 addresses and pick an advertise host.
pub fn local_advertise_ipv4() -> Option<Ipv4Addr> {
    let Ok(ifaces) = local_ip_address::list_afinet_netifas() else {
        return None;
    };
    let v4: Vec<(String, Ipv4Addr)> = ifaces
        .into_iter()
        .filter_map(|(name, addr)| match addr {
            IpAddr::V4(v4) => Some((name, v4)),
            IpAddr::V6(_) => None,
        })
        .collect();
    if v4.is_empty() {
        return None;
    }
    select_advertise_ipv4_with_interfaces(&v4)
}

/// Same as [`local_advertise_ipv4`] but as a display string for UI / logs.
pub fn local_advertise_host() -> Option<String> {
    local_advertise_ipv4().map(|ip| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_private_over_public() {
        let addrs = [
            "8.8.8.8".parse().unwrap(),
            "10.0.0.5".parse().unwrap(),
            "192.168.1.20".parse().unwrap(),
        ];
        assert_eq!(
            select_advertise_ipv4(&addrs),
            Some("192.168.1.20".parse().unwrap())
        );
    }

    #[test]
    fn skips_loopback_link_local_and_unspecified() {
        let addrs = [
            "127.0.0.1".parse().unwrap(),
            "0.0.0.0".parse().unwrap(),
            "169.254.10.2".parse().unwrap(),
            "192.168.0.10".parse().unwrap(),
        ];
        assert_eq!(
            select_advertise_ipv4(&addrs),
            Some("192.168.0.10".parse().unwrap())
        );
    }

    #[test]
    fn empty_or_only_unusable_returns_none() {
        assert_eq!(select_advertise_ipv4(&[]), None);
        let bad = [
            "127.0.0.1".parse().unwrap(),
            "169.254.1.1".parse().unwrap(),
            "0.0.0.0".parse().unwrap(),
        ];
        assert_eq!(select_advertise_ipv4(&bad), None);
    }

    #[test]
    fn skips_multicast_and_falls_back_to_public() {
        let addrs = ["224.0.0.251".parse().unwrap(), "8.8.8.8".parse().unwrap()];
        assert_eq!(
            select_advertise_ipv4(&addrs),
            Some("8.8.8.8".parse().unwrap())
        );
    }

    #[test]
    fn prefers_192_168_over_docker_bridge_172_18() {
        let ifaces = [
            ("vEthernet (WSL)".into(), "172.18.0.1".parse().unwrap()),
            ("Wi-Fi".into(), "192.168.1.108".parse().unwrap()),
        ];
        assert_eq!(
            select_advertise_ipv4_with_interfaces(&ifaces),
            Some("192.168.1.108".parse().unwrap())
        );
    }

    #[test]
    fn virtual_adapter_only_is_not_advertised_to_phones() {
        let ifaces = [("DockerNAT".into(), "172.18.0.1".parse().unwrap())];
        assert_eq!(select_advertise_ipv4_with_interfaces(&ifaces), None);
    }

    #[test]
    fn wifi_beats_ethernet_when_both_present() {
        let ifaces = [
            ("Ethernet".into(), "10.0.0.5".parse().unwrap()),
            ("Wi-Fi".into(), "192.168.0.20".parse().unwrap()),
        ];
        assert_eq!(
            select_advertise_ipv4_with_interfaces(&ifaces),
            Some("192.168.0.20".parse().unwrap())
        );
    }

    #[test]
    fn excludes_macos_and_windows_vpn_interfaces() {
        let ifaces = [
            ("utun4".into(), "10.20.0.2".parse().unwrap()),
            ("WireGuard Tunnel".into(), "192.168.90.2".parse().unwrap()),
            ("en0".into(), "192.168.8.110".parse().unwrap()),
        ];
        assert_eq!(
            select_advertise_ipv4_with_interfaces(&ifaces),
            Some("192.168.8.110".parse().unwrap())
        );
    }

    #[test]
    fn discovery_ports_match_wix_firewall() {
        assert_eq!(DEFAULT_QUIC_PORT, 4433);
        let wxs = include_str!("../../../installers/windows/picoo-camera.wxs");
        assert!(
            wxs.contains("Port=\"4433\""),
            "WiX FirewallException Port must equal DEFAULT_QUIC_PORT ({DEFAULT_QUIC_PORT})"
        );
        assert!(
            wxs.contains("Port=\"5353\""),
            "WiX FirewallException must allow mDNS/DNS-SD discovery on UDP 5353"
        );
    }
}
