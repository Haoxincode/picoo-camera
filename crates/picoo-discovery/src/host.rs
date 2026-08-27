//! LAN advertise / QR host selection — REQ-PICOO-DISCOVERY-001 / 003.
//!
//! Receiver binds `0.0.0.0` but must advertise a reachable unicast IPv4 to phones
//! on the same LAN (never loopback / unspecified / link-local).

use std::net::{IpAddr, Ipv4Addr};

/// Default QUIC UDP port (aligned with WiX FirewallException).
pub const DEFAULT_QUIC_PORT: u16 = 4433;

/// Choose the best IPv4 to put in mDNS A records and QR payloads.
///
/// Preference: RFC1918 private → other global unicast → none.
/// Skips loopback, unspecified, and link-local (169.254/16).
pub fn select_advertise_ipv4(candidates: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    let usable: Vec<Ipv4Addr> = candidates
        .iter()
        .copied()
        .filter(|ip| is_advertise_candidate(*ip))
        .collect();
    usable
        .iter()
        .copied()
        .find(|ip| ip.is_private())
        .or_else(|| usable.first().copied())
}

fn is_advertise_candidate(ip: Ipv4Addr) -> bool {
    !ip.is_unspecified() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_multicast()
}

/// Enumerate local interface IPv4 addresses and pick an advertise host.
pub fn local_advertise_ipv4() -> Option<Ipv4Addr> {
    let addrs = local_ipv4_addrs();
    select_advertise_ipv4(&addrs)
}

/// Same as [`local_advertise_ipv4`] but as a display string for QR / logs.
pub fn local_advertise_host() -> Option<String> {
    local_advertise_ipv4().map(|ip| ip.to_string())
}

fn local_ipv4_addrs() -> Vec<Ipv4Addr> {
    let Ok(ifaces) = local_ip_address::list_afinet_netifas() else {
        return Vec::new();
    };
    ifaces
        .into_iter()
        .filter_map(|(_name, addr)| match addr {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        })
        .collect()
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
        // First private in list order among filtered — 10.0.0.5 appears before 192.168.
        assert_eq!(
            select_advertise_ipv4(&addrs),
            Some("10.0.0.5".parse().unwrap())
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
    fn first_private_in_list_order_wins() {
        let addrs = ["192.168.1.20".parse().unwrap(), "10.0.0.5".parse().unwrap()];
        assert_eq!(
            select_advertise_ipv4(&addrs),
            Some("192.168.1.20".parse().unwrap())
        );
    }

    #[test]
    fn default_quic_port_matches_wix_firewall() {
        assert_eq!(DEFAULT_QUIC_PORT, 4433);
        // Keep in sync with installers/windows/picoo-camera.wxs FirewallException.
        let wxs = include_str!("../../../installers/windows/picoo-camera.wxs");
        assert!(
            wxs.contains("Port=\"4433\""),
            "WiX FirewallException Port must equal DEFAULT_QUIC_PORT ({DEFAULT_QUIC_PORT})"
        );
    }
}
