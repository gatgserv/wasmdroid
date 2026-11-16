//! Per-instance inbound-only virtual network. No host network implementation is
//! reachable. Bound sockets also deny connect(), which would otherwise bypass
//! VirtualNetworking::connect_tcp and use upstream's first-listener fallback.
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use wasmer_wasix::virtual_net::{
    self, LoopbackNetworking, NetworkError, VirtualNetworking, VirtualTcpBoundSocket,
    VirtualTcpListener, VirtualTcpSocket,
};

#[derive(Debug, Clone)]
pub(crate) struct InboundNetwork {
    inner: LoopbackNetworking,
    target: SocketAddr,
    listening: Arc<AtomicBool>,
}

impl InboundNetwork {
    pub fn new(port: u16) -> Self {
        Self {
            inner: LoopbackNetworking::new(),
            target: ([127, 0, 0, 1], port).into(),
            listening: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn connect_forwarder(&self) -> Option<Box<dyn VirtualTcpSocket + Sync>> {
        if !self.listening.load(Ordering::Acquire) {
            return None;
        }
        // Only this exact target may ever listen in this network. Never pass a
        // caller-supplied address to loopback_connect_to (upstream has fallback).
        self.inner
            .loopback_connect_to(([127, 0, 0, 1], 49152).into(), self.target)
            .map(|socket| Box::new(socket) as Box<dyn VirtualTcpSocket + Sync>)
    }
}

#[async_trait::async_trait]
impl VirtualNetworking for InboundNetwork {
    async fn listen_tcp(
        &self,
        addr: SocketAddr,
        v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> virtual_net::Result<Box<dyn VirtualTcpListener + Sync>> {
        self.bind_tcp(addr, v6, reuse_port, reuse_addr)
            .await?
            .listen()
    }
    async fn bind_tcp(
        &self,
        addr: SocketAddr,
        v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> virtual_net::Result<Box<dyn VirtualTcpBoundSocket + Sync>> {
        if addr != self.target {
            return Err(NetworkError::PermissionDenied);
        }
        Ok(Box::new(InboundBound {
            inner: self
                .inner
                .bind_tcp(addr, v6, reuse_port, reuse_addr)
                .await?,
            listening: self.listening.clone(),
        }))
    }
}

#[derive(Debug)]
struct InboundBound {
    inner: Box<dyn VirtualTcpBoundSocket + Sync>,
    listening: Arc<AtomicBool>,
}
impl VirtualTcpBoundSocket for InboundBound {
    fn addr_local(&self) -> virtual_net::Result<SocketAddr> {
        self.inner.addr_local()
    }
    fn listen(&mut self) -> virtual_net::Result<Box<dyn VirtualTcpListener + Sync>> {
        let listener = self.inner.listen()?;
        self.listening.store(true, Ordering::Release);
        Ok(listener)
    }
    fn connect(&mut self, _: SocketAddr) -> virtual_net::Result<Box<dyn VirtualTcpSocket + Sync>> {
        Err(NetworkError::PermissionDenied)
    }
    fn set_ttl(&mut self, ttl: u32) -> virtual_net::Result<()> {
        self.inner.set_ttl(ttl)
    }
    fn ttl(&self) -> virtual_net::Result<u32> {
        self.inner.ttl()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn denies_egress_wrong_bind_and_cross_instance_fallback() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let first = InboundNetwork::new(8080);
            let second = InboundNetwork::new(8080);
            assert!(first.connect_forwarder().is_none());
            for address in ["0.0.0.0:8080", "192.0.2.1:8080", "127.0.0.1:8081"] {
                assert!(
                    first
                        .bind_tcp(address.parse().unwrap(), false, false, false)
                        .await
                        .is_err()
                );
            }
            let mut bound = first
                .bind_tcp(first.target, false, false, false)
                .await
                .unwrap();
            for peer in ["127.0.0.1:80", "192.0.2.1:80", "127.0.0.1:8080"] {
                assert!(bound.connect(peer.parse().unwrap()).is_err());
                assert!(
                    first
                        .connect_tcp(first.target, peer.parse().unwrap())
                        .await
                        .is_err()
                );
            }
            assert!(first.resolve("example.com", Some(443), None).await.is_err());
            assert!(first.bind_udp(first.target, false, false).await.is_err());
            let _listener = bound.listen().unwrap();
            assert!(first.connect_forwarder().is_some());
            assert!(second.connect_forwarder().is_none());
        });
    }
}
