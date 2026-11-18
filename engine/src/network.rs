//! Runtime-neutral host port ownership. Keep the listener until forwarding stops;
//! probing a free port and reopening it would introduce an allocation race.
pub mod egress;
pub mod grants;
pub mod lan;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};

use crate::{
    EngineError, Result,
    api::{NetworkMode, PortBinding, Protocol},
    state::{BrowserOriginCapture, StateDb},
};

#[derive(Debug)]
pub struct PortLease {
    listener: TcpListener,
    instance_port: u16,
}

impl PortLease {
    /// Stable per-private-profile origins for normal application workers. The
    /// saved number is never connected to as authority: bind the real listener
    /// and retain it through publication and forwarding. Removed apps retain
    /// their history; other OS processes can still occupy stopped ports.
    pub fn reserve_browser_origins(
        state: &StateDb,
        capture: &BrowserOriginCapture,
    ) -> Result<Vec<Self>> {
        const FIRST_ALLOCATION_ATTEMPTS: usize = 32;
        let saved = state.saved_origin_ports(capture)?;
        let mut leases = Vec::new();
        let mut actual = Vec::new();
        for (requested, saved) in capture.bindings().iter().zip(saved) {
            if saved.is_some_and(|host| requested.host_port != 0 && requested.host_port != host) {
                return Err(EngineError::Unsupported(
                    "changing a saved browser origin host port requires explicit origin-change review".into(),
                ));
            }
            let mut binding = requested.clone();
            binding.host_port = saved.unwrap_or(requested.host_port);
            let attempts = if binding.host_port == 0 {
                FIRST_ALLOCATION_ATTEMPTS
            } else {
                1
            };
            let mut chosen = None;
            for _ in 0..attempts {
                // Busy saved/explicit ports fail immediately. There is no
                // probe-close-rebind and no fallback to another known origin.
                let lease = Self::reserve(&binding, NetworkMode::Loopback)?;
                let host = lease.host_address()?.port();
                if state.origin_candidate_available(capture, binding.instance_port, host)? {
                    let mut published = requested.clone();
                    published.host_port = host;
                    actual.push(published);
                    chosen = Some(lease);
                    break;
                }
                // Only an unpublished first automatic allocation can retry a
                // number that history, rather than the OS, says is unavailable.
            }
            leases.push(chosen.ok_or_else(|| {
                EngineError::Conflict(
                    "no unclaimed browser origin port within allocation budget".into(),
                )
            })?);
        }
        state.claim_browser_origins(capture, &actual)?;
        Ok(leases)
    }

    /// Revalidate the opaque native capture while every actual listener is
    /// still held. This does not replace common-owner generation admission.
    pub fn validate_browser_origins(
        state: &StateDb,
        capture: &BrowserOriginCapture,
        leases: &[Self],
    ) -> Result<()> {
        let actual = leases
            .iter()
            .map(|lease| {
                Ok(PortBinding {
                    host_port: lease.host_address()?.port(),
                    instance_port: lease.instance_port,
                    protocol: Protocol::Tcp,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        state.validate_browser_origin_listeners(capture, &actual)
    }

    pub fn reserve(binding: &PortBinding, mode: NetworkMode) -> Result<Self> {
        if binding.protocol != Protocol::Tcp || mode != NetworkMode::Loopback {
            return Err(EngineError::Unsupported(
                "only loopback TCP forwarding is available".into(),
            ));
        }
        if binding.instance_port == 0 {
            return Err(EngineError::Invalid("instance port cannot be zero".into()));
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, binding.host_port))?;
        Ok(Self {
            listener,
            instance_port: binding.instance_port,
        })
    }

    pub fn host_address(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }
    pub fn instance_port(&self) -> u16 {
        self.instance_port
    }
    pub fn into_listener(self) -> TcpListener {
        self.listener
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_loopback_without_a_close_reopen_race() {
        let binding = PortBinding {
            host_port: 0,
            instance_port: 8080,
            protocol: Protocol::Tcp,
        };
        let lease = PortLease::reserve(&binding, NetworkMode::Loopback).unwrap();
        let address = lease.host_address().unwrap();
        assert_eq!(address.ip(), Ipv4Addr::LOCALHOST);
        assert_ne!(address.port(), 0);
        assert!(TcpListener::bind(address).is_err());
        drop(lease);
        assert!(TcpListener::bind(address).is_ok());
    }

    #[test]
    fn rejects_unsupported_policy_instead_of_broadening_it() {
        let mut binding = PortBinding {
            host_port: 0,
            instance_port: 8080,
            protocol: Protocol::Tcp,
        };
        for mode in [NetworkMode::Lan, NetworkMode::None] {
            assert!(matches!(
                PortLease::reserve(&binding, mode),
                Err(EngineError::Unsupported(_))
            ));
        }
        binding.protocol = Protocol::Udp;
        assert!(matches!(
            PortLease::reserve(&binding, NetworkMode::Loopback),
            Err(EngineError::Unsupported(_))
        ));
        binding.protocol = Protocol::Tcp;
        binding.instance_port = 0;
        assert!(PortLease::reserve(&binding, NetworkMode::Loopback).is_err());
    }
}
