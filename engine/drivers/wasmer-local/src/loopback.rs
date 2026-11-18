//! Per-instance inbound-only virtual network. No host network implementation is
//! reachable. Bound sockets also deny connect(), so only the explicitly
//! reserved host forwarder can reach the probe's guest listener.
use crate::{http_execution::ForwardTarget, service_network::ServiceNetwork};
use std::net::SocketAddr;
use wasmer_wasix::virtual_net::{
    self, NetworkError, VirtualNetworking, VirtualTcpBoundSocket, VirtualTcpListener,
    VirtualTcpSocket,
};

#[derive(Debug, Clone)]
pub(crate) struct InboundNetwork {
    inner: ServiceNetwork,
    target: SocketAddr,
}

impl InboundNetwork {
    pub fn new(port: u16) -> Self {
        Self {
            inner: ServiceNetwork::new(true),
            target: ([127, 0, 0, 1], port).into(),
        }
    }

    pub fn connect_forwarder(&self) -> Option<Box<dyn VirtualTcpSocket + Sync>> {
        self.inner.ingress(self.target.port()).connect_forwarder()
    }

    pub fn close(&self) {
        self.inner.close();
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
        }))
    }
}

#[derive(Debug)]
struct InboundBound {
    inner: Box<dyn VirtualTcpBoundSocket + Sync>,
}
impl VirtualTcpBoundSocket for InboundBound {
    fn addr_local(&self) -> virtual_net::Result<SocketAddr> {
        self.inner.addr_local()
    }
    fn listen(&mut self) -> virtual_net::Result<Box<dyn VirtualTcpListener + Sync>> {
        self.inner.listen()
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

    #[tokio::test]
    async fn closing_one_probe_revokes_its_sockets_and_preserves_the_other_probe() {
        let first = InboundNetwork::new(8080);
        let second = InboundNetwork::new(8080);
        let mut first_listener = first
            .listen_tcp(first.target, false, false, false)
            .await
            .unwrap();
        let mut second_listener = second
            .listen_tcp(second.target, false, false, false)
            .await
            .unwrap();
        let mut first_client = first.connect_forwarder().unwrap();
        let (mut first_peer, _) = first_listener.try_accept().unwrap();
        let mut second_client = second.connect_forwarder().unwrap();
        let (mut second_peer, _) = second_listener.try_accept().unwrap();

        first.close();

        assert!(first.connect_forwarder().is_none());
        assert!(matches!(
            first_listener.try_accept(),
            Err(NetworkError::ConnectionAborted)
        ));
        assert_eq!(
            first_client.try_send(b"closed"),
            Err(NetworkError::NotConnected)
        );
        assert_eq!(
            first_peer.try_send(b"closed"),
            Err(NetworkError::NotConnected)
        );
        assert_eq!(second_client.try_send(b"open").unwrap(), 4);
        let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 4];
        assert_eq!(
            second_peer.try_recv(&mut buffer, false).unwrap(),
            buffer.len()
        );
    }
}
