//! Selected numeric TCP-peer policy using actual grants and captured OS metadata.
//! No resolver, route selection, serde authority or Wasmer dependency. Hidden
//! router/VPN translation and remote proxies are not a physical-isolation claim.
use crate::{EngineError, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NetworkPrefix {
    address: IpAddr,
    length: u8,
}
impl std::fmt::Debug for NetworkPrefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NetworkPrefix(<private>)")
    }
}
impl NetworkPrefix {
    pub fn new(address: IpAddr, length: u8) -> Result<Self> {
        let maximum = if address.is_ipv4() { 32 } else { 128 };
        if length > maximum {
            return Err(invalid());
        }
        let value = Self { address, length };
        if value.network() != address {
            return Err(invalid());
        }
        Ok(value)
    }
    pub fn parse(value: &str) -> Result<Self> {
        if value.len() > 64 || !value.is_ascii() {
            return Err(invalid());
        }
        let (address, length) = value.split_once('/').ok_or_else(invalid)?;
        let length: u8 = length.parse().map_err(|_| invalid())?;
        if value.rsplit('/').next() != Some(length.to_string().as_str()) {
            return Err(invalid());
        }
        Self::new(address.parse().map_err(|_| invalid())?, length)
    }
    pub fn address(&self) -> IpAddr {
        self.address
    }
    pub fn length(&self) -> u8 {
        self.length
    }
    fn network(&self) -> IpAddr {
        match self.address {
            IpAddr::V4(ip) => IpAddr::V4(Ipv4Addr::from(u32::from(ip) & mask4(self.length))),
            IpAddr::V6(ip) => IpAddr::V6(Ipv6Addr::from(u128::from(ip) & mask6(self.length))),
        }
    }
    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.address, address) {
            (IpAddr::V4(prefix), IpAddr::V4(ip)) => {
                u32::from(prefix) == u32::from(ip) & mask4(self.length)
            }
            (IpAddr::V6(prefix), IpAddr::V6(ip)) => {
                u128::from(prefix) == u128::from(ip) & mask6(self.length)
            }
            _ => false,
        }
    }
}
const fn mask4(length: u8) -> u32 {
    if length == 0 {
        0
    } else {
        u32::MAX << (32 - length)
    }
}
const fn mask6(length: u8) -> u128 {
    if length == 0 {
        0
    } else {
        u128::MAX << (128 - length)
    }
}
fn invalid() -> EngineError {
    EngineError::Invalid("invalid captured outbound network context".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    HostAddress,
    Loopback,
    Unspecified,
    Multicast,
    Broadcast,
    VirtualNamespace,
    SpecialPurpose,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedReason {
    Ipv4Alias,
    LinkLocal,
    ScopedAddress,
    Transition,
    MalformedTranslation,
    OnLinkGlobal,
    AddressSpace,
    Overlay,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationClass {
    Internet,
    Lan,
    Denied(DenyReason),
    Unsupported(UnsupportedReason),
}

/// Private immutable snapshot, never a guest/wire grant. The host must validate
/// its epoch and invalidate the runtime capture when actual metadata changes.
#[derive(Clone)]
pub struct EgressContext {
    own: Vec<IpAddr>,
    on_link: Vec<NetworkPrefix>,
    nat64: Vec<NetworkPrefix>,
}
impl std::fmt::Debug for EgressContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EgressContext(<private>)")
    }
}
impl EgressContext {
    pub fn new(
        own_addresses: Vec<IpAddr>,
        on_link: Vec<NetworkPrefix>,
        nat64: Vec<NetworkPrefix>,
    ) -> Result<Self> {
        if own_addresses.len() > 128 || on_link.len() > 128 || nat64.len() > 8 {
            return Err(invalid());
        }
        if on_link.iter().any(|prefix| prefix.length == 0) {
            return Err(invalid());
        }
        let mut own = Vec::new();
        for address in own_addresses {
            let address = address.to_canonical();
            if !own.contains(&address) {
                own.push(address);
            }
        }
        let mut links = Vec::new();
        for prefix in on_link {
            if prefix.address.to_canonical() != prefix.address {
                return Err(invalid());
            }
            if !links.contains(&prefix) {
                links.push(prefix);
            }
        }
        for (index, prefix) in nat64.iter().enumerate() {
            if !prefix.address.is_ipv6()
                || ![32, 40, 48, 56, 64, 96].contains(&prefix.length)
                || nat64[..index]
                    .iter()
                    .any(|prior| prior.contains(prefix.address) || prefix.contains(prior.address))
                || (prefix.contains(well_known().address) && *prefix != well_known())
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            own,
            on_link: links,
            nat64,
        })
    }
    fn translation(&self, address: IpAddr) -> Option<NetworkPrefix> {
        self.nat64
            .iter()
            .copied()
            .find(|prefix| prefix.contains(address))
            .or_else(|| well_known().contains(address).then(well_known))
    }
    fn hard_denial(&self, address: IpAddr) -> Option<DenyReason> {
        let canonical = address.to_canonical();
        if canonical.is_unspecified() {
            return Some(DenyReason::Unspecified);
        }
        if canonical.is_loopback() {
            return Some(DenyReason::Loopback);
        }
        if self.own.contains(&canonical) {
            return Some(DenyReason::HostAddress);
        }
        if canonical.is_multicast() {
            return Some(DenyReason::Multicast);
        }
        if let IpAddr::V4(ip) = canonical {
            let raw = u32::from(ip);
            if ip.octets()[..3] == [10, 203, 0] {
                return Some(DenyReason::VirtualNamespace);
            }
            if ip.is_broadcast()
                || self.on_link.iter().any(|prefix| {
                    if let IpAddr::V4(network) = prefix.address {
                        prefix.length <= 30 && raw == (u32::from(network) | !mask4(prefix.length))
                    } else {
                        false
                    }
                })
            {
                return Some(DenyReason::Broadcast);
            }
        }
        None
    }
}

#[derive(Clone)]
pub struct EgressPolicy {
    internet: bool,
    lan: bool,
    context: EgressContext,
}
impl std::fmt::Debug for EgressPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EgressPolicy(<private>)")
    }
}
impl EgressPolicy {
    /// Booleans come from the actual current native specification, not a wire
    /// request or requested catalog permissions. Scope/epoch checks are external.
    pub fn new(internet: bool, lan: bool, context: EgressContext) -> Self {
        Self {
            internet,
            lan,
            context,
        }
    }
    pub fn classify(&self, address: IpAddr) -> DestinationClass {
        if let Some(reason) = self.context.hard_denial(address) {
            return DestinationClass::Denied(reason);
        }
        match address {
            IpAddr::V4(ip) => self.finish_class(address, class4(ip)),
            IpAddr::V6(ip) => {
                if ip.to_ipv4_mapped().is_some() || u128::from(ip) >> 32 == 0 {
                    return DestinationClass::Unsupported(UnsupportedReason::Ipv4Alias);
                }
                if let Some(prefix) = self.context.translation(address) {
                    let outer = class6(ip);
                    if matches!(outer, DestinationClass::Denied(_)) {
                        return outer;
                    }
                    let local_translation = NetworkPrefix {
                        address: IpAddr::V6(Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0)),
                        length: 48,
                    };
                    if matches!(outer, DestinationClass::Unsupported(_))
                        && prefix != well_known()
                        && !local_translation.contains(address)
                    {
                        return outer;
                    }
                    if self
                        .context
                        .on_link
                        .iter()
                        .any(|link| link.contains(address))
                        && outer == DestinationClass::Internet
                    {
                        return DestinationClass::Unsupported(UnsupportedReason::OnLinkGlobal);
                    }
                    let Some(embedded) = embedded_v4(ip, prefix.length) else {
                        return DestinationClass::Unsupported(
                            UnsupportedReason::MalformedTranslation,
                        );
                    };
                    if let Some(reason) = self.context.hard_denial(embedded.into()) {
                        return DestinationClass::Denied(reason);
                    }
                    let effective = self.finish_class(embedded.into(), class4(embedded));
                    // RFC6052 forbids WKP with non-global IPv4 even with LAN on.
                    if prefix == well_known() && effective != DestinationClass::Internet {
                        return DestinationClass::Denied(DenyReason::SpecialPurpose);
                    }
                    return effective;
                }
                self.finish_class(address, class6(ip))
            }
        }
    }
    fn finish_class(&self, address: IpAddr, class: DestinationClass) -> DestinationClass {
        if class == DestinationClass::Internet
            && self
                .context
                .on_link
                .iter()
                .any(|prefix| prefix.contains(address))
        {
            DestinationClass::Unsupported(UnsupportedReason::OnLinkGlobal)
        } else {
            class
        }
    }
    pub fn authorize_dns(&self) -> Result<()> {
        if self.internet || self.lan {
            Ok(())
        } else {
            Err(EngineError::Unsupported(
                "outbound DNS permission is absent".into(),
            ))
        }
    }
    pub fn authorize_ip(&self, address: IpAddr) -> Result<()> {
        // A known translator reached via ULA additionally needs LAN permission;
        // LAN alone still cannot acquire a globally addressed embedded peer.
        if self.context.translation(address).is_some()
            && matches!(address,IpAddr::V6(ip) if class6(ip)==DestinationClass::Lan)
            && !self.lan
        {
            return Err(EngineError::Unsupported(
                "outbound LAN permission is absent".into(),
            ));
        }
        match self.classify(address) {
            DestinationClass::Internet if self.internet => Ok(()),
            DestinationClass::Lan if self.lan => Ok(()),
            DestinationClass::Unsupported(_) => Err(EngineError::Unsupported(
                "outbound destination form is not supported".into(),
            )),
            _ => Err(EngineError::Unsupported(
                "outbound destination is not permitted".into(),
            )),
        }
    }
    pub fn authorize_tcp(&self, peer: SocketAddr) -> Result<()> {
        if peer.port() == 0 {
            return Err(EngineError::Invalid(
                "outbound TCP port cannot be zero".into(),
            ));
        }
        if matches!(peer,SocketAddr::V6(address) if address.scope_id()!=0) {
            return Err(EngineError::Unsupported(
                "scoped outbound TCP destination is not supported".into(),
            ));
        }
        self.authorize_ip(peer.ip())
    }
}
fn well_known() -> NetworkPrefix {
    NetworkPrefix {
        address: IpAddr::V6(Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0)),
        length: 96,
    }
}
fn embedded_v4(address: Ipv6Addr, length: u8) -> Option<Ipv4Addr> {
    let bytes = address.octets();
    if length == 96 {
        return Some(Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]));
    }
    if bytes[8] != 0 {
        return None;
    }
    let mut result = [0; 4];
    let start = usize::from(length / 8);
    let mut end = 0;
    for (index, octet) in result.iter_mut().enumerate() {
        let offset = start + index;
        let offset = if offset >= 8 { offset + 1 } else { offset };
        *octet = bytes[offset];
        end = offset + 1;
    }
    if bytes[end..].iter().any(|byte| *byte != 0) {
        return None;
    }
    Some(result.into())
}

const fn v4(a: u8, b: u8, c: u8, d: u8) -> u32 {
    u32::from_be_bytes([a, b, c, d])
}
const fn v6(a: u16, b: u16, c: u16, d: u16) -> u128 {
    ((a as u128) << 112) | ((b as u128) << 96) | ((c as u128) << 80) | ((d as u128) << 64)
}
use DestinationClass::{Denied as D, Internet as I, Lan as L, Unsupported as U};
// Reviewed IANA registries, updated2025-10-09, fetched2026-11-17:
// https://www.iana.org/assignments/iana-ipv4-special-registry/
// https://www.iana.org/assignments/iana-ipv6-special-registry/
// Most-specific matching wins. Overlay/transition entries are explicitly not
// supported ordinary TCP peers even where registry reachability is true/N/A.
const SPECIAL4: &[(u32, u8, DestinationClass)] = &[
    (v4(0, 0, 0, 0), 8, D(DenyReason::SpecialPurpose)),
    (v4(10, 0, 0, 0), 8, L),
    (v4(100, 64, 0, 0), 10, D(DenyReason::SpecialPurpose)),
    (v4(127, 0, 0, 0), 8, D(DenyReason::Loopback)),
    (v4(169, 254, 0, 0), 16, U(UnsupportedReason::LinkLocal)),
    (v4(172, 16, 0, 0), 12, L),
    (v4(192, 0, 0, 0), 24, D(DenyReason::SpecialPurpose)),
    (v4(192, 0, 0, 9), 32, I),
    (v4(192, 0, 0, 10), 32, I),
    (v4(192, 0, 2, 0), 24, D(DenyReason::SpecialPurpose)),
    (v4(192, 31, 196, 0), 24, I),
    (v4(192, 52, 193, 0), 24, I),
    (v4(192, 88, 99, 0), 24, U(UnsupportedReason::Transition)),
    (v4(192, 168, 0, 0), 16, L),
    (v4(192, 175, 48, 0), 24, I),
    (v4(198, 18, 0, 0), 15, D(DenyReason::SpecialPurpose)),
    (v4(198, 51, 100, 0), 24, D(DenyReason::SpecialPurpose)),
    (v4(203, 0, 113, 0), 24, D(DenyReason::SpecialPurpose)),
    (v4(224, 0, 0, 0), 4, D(DenyReason::Multicast)),
    (v4(240, 0, 0, 0), 4, D(DenyReason::SpecialPurpose)),
];
const SPECIAL6: &[(u128, u8, DestinationClass)] = &[
    (v6(0x64, 0xff9b, 0, 0), 96, U(UnsupportedReason::Transition)),
    (v6(0x64, 0xff9b, 1, 0), 48, U(UnsupportedReason::Transition)),
    (v6(0x100, 0, 0, 0), 64, D(DenyReason::SpecialPurpose)),
    (v6(0x100, 0, 0, 1), 64, D(DenyReason::SpecialPurpose)),
    (v6(0x2001, 0, 0, 0), 23, D(DenyReason::SpecialPurpose)),
    (v6(0x2001, 0, 0, 0), 32, U(UnsupportedReason::Transition)),
    (v6(0x2001, 1, 0, 0) | 1, 128, I),
    (v6(0x2001, 1, 0, 0) | 2, 128, I),
    (v6(0x2001, 1, 0, 0) | 3, 128, I),
    (v6(0x2001, 2, 0, 0), 48, D(DenyReason::SpecialPurpose)),
    (v6(0x2001, 3, 0, 0), 32, I),
    (v6(0x2001, 4, 0x112, 0), 48, I),
    (v6(0x2001, 0x10, 0, 0), 28, U(UnsupportedReason::Overlay)),
    (v6(0x2001, 0x20, 0, 0), 28, U(UnsupportedReason::Overlay)),
    (v6(0x2001, 0x30, 0, 0), 28, U(UnsupportedReason::Overlay)),
    (v6(0x2001, 0xdb8, 0, 0), 32, D(DenyReason::SpecialPurpose)),
    (v6(0x2002, 0, 0, 0), 16, U(UnsupportedReason::Transition)),
    (v6(0x2620, 0x4f, 0x8000, 0), 48, I),
    (v6(0x3fff, 0, 0, 0), 20, D(DenyReason::SpecialPurpose)),
    (v6(0x5f00, 0, 0, 0), 16, D(DenyReason::SpecialPurpose)),
    (v6(0xfc00, 0, 0, 0), 7, L),
    (v6(0xfe80, 0, 0, 0), 10, U(UnsupportedReason::LinkLocal)),
    (v6(0xfec0, 0, 0, 0), 10, U(UnsupportedReason::AddressSpace)),
    (v6(0xff00, 0, 0, 0), 8, D(DenyReason::Multicast)),
];
fn class4(ip: Ipv4Addr) -> DestinationClass {
    let ip = u32::from(ip);
    SPECIAL4
        .iter()
        .filter(|(prefix, length, _)| ip & mask4(*length) == *prefix)
        .max_by_key(|(_, length, _)| *length)
        .map(|(_, _, kind)| *kind)
        .unwrap_or(I)
}
fn class6(ip: Ipv6Addr) -> DestinationClass {
    let ip = u128::from(ip);
    SPECIAL6
        .iter()
        .filter(|(prefix, length, _)| ip & mask6(*length) == *prefix)
        .max_by_key(|(_, length, _)| *length)
        .map(|(_, _, kind)| *kind)
        .unwrap_or_else(|| {
            if ip & mask6(3) == v6(0x2000, 0, 0, 0) {
                I
            } else {
                U(UnsupportedReason::AddressSpace)
            }
        })
}
