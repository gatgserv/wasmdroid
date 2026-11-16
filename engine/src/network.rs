//! Runtime-neutral host port ownership. Keep the listener until forwarding stops;
//! probing a free port and reopening it would introduce an allocation race.
use std::net::{Ipv4Addr, SocketAddr, TcpListener};

use crate::{
    EngineError, Result,
    api::{NetworkMode, PortBinding, Protocol},
};

#[derive(Debug)]
pub struct PortLease {
    listener: TcpListener,
    instance_port: u16,
}

impl PortLease {
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
