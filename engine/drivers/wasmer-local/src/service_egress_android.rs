//! Same-worker Android host adapter. No JNI/Network/default policy is inferred.
//! Root host owns the actual immutable Network and installs the revoke handle
//! before long startup. Blocking work retains its own native ownership lease.
use crate::service_egress::{
    BindFuture, EgressResolution, EgressScope, InvalidationFuture, ResolveFuture, SelectedNetwork,
};
use std::{net::IpAddr, sync::Arc};
use wasmdroid_engine::{EngineError, Result, network::lan::LanRevocation};

pub trait AndroidEgressHost: Send + Sync + 'static {
    /// Atomic/read-only actual-capture observation; never performs network IO.
    fn current(&self) -> bool;
    /// Descriptor is owned by this call's retained native task, never adopted.
    fn bind_socket(&self, descriptor: i32) -> bool;
    /// Must use that SAME captured Network; bounded numeric answers only.
    fn resolve_name(&self, ascii: &str) -> Result<Vec<IpAddr>>;
}
pub struct AndroidSelectedNetwork {
    scope: EgressScope,
    host: Arc<dyn AndroidEgressHost>,
    revoked: LanRevocation,
}
impl std::fmt::Debug for AndroidSelectedNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AndroidSelectedNetwork(<private capture>)")
    }
}
impl AndroidSelectedNetwork {
    pub fn new(scope: EgressScope, host: Arc<dyn AndroidEgressHost>) -> Result<Arc<Self>> {
        scope.native_owner()?;
        #[cfg(not(unix))]
        {
            let _ = (scope, host);
            Err(EngineError::Unsupported(
                "outbound socket adapter requires Android/Unix".into(),
            ))
        }
        #[cfg(unix)]
        {
            if !host.current() {
                return Err(refused());
            }
            Ok(Arc::new(Self {
                scope,
                host,
                revoked: LanRevocation::new(),
            }))
        }
    }
    /// Immediate signal; actual provider/stream cleanup remains manager-owned.
    pub fn revoke(&self) {
        self.revoked.revoke();
    }
    fn check(&self, scope: &EgressScope) -> Result<()> {
        if scope != &self.scope || !self.current() {
            Err(refused())
        } else {
            Ok(())
        }
    }
}
impl SelectedNetwork for AndroidSelectedNetwork {
    fn scope(&self) -> &EgressScope {
        &self.scope
    }
    fn current(&self) -> bool {
        // WASIX can call this from a guest fiber. Only the native revocation
        // signal is safe here. The platform signals it through the revoker.
        !self.revoked.is_revoked()
    }
    fn invalidated(&self) -> InvalidationFuture<'_> {
        Box::pin(self.revoked.revoked())
    }
    fn bind<'a>(
        &'a self,
        scope: &'a EgressScope,
        socket: &'a tokio::net::TcpSocket,
    ) -> BindFuture<'a> {
        Box::pin(async move {
            self.check(scope)?;
            #[cfg(not(unix))]
            {
                let _ = socket;
                Err(EngineError::Unsupported(
                    "outbound socket binding requires Android/Unix".into(),
                ))
            }
            #[cfg(unix)]
            {
                use std::os::fd::{AsFd, AsRawFd};
                let descriptor = socket.as_fd().try_clone_to_owned()?;
                let owner = self.scope.native_owner()?;
                let host = self.host.clone();
                let revoked = self.revoked.clone();
                tokio::task::spawn_blocking(move || {
                    let _owner = owner;
                    if !provider_current(host.as_ref(), &revoked) {
                        return Err(refused());
                    }
                    let bound = host.bind_socket(descriptor.as_raw_fd());
                    drop(descriptor);
                    if !provider_current(host.as_ref(), &revoked) || !bound {
                        Err(refused())
                    } else {
                        Ok(())
                    }
                })
                .await
                .map_err(|_| {
                    EngineError::Conflict("outbound binder ended without a receipt".into())
                })??;
                self.check(scope)
            }
        })
    }
    fn resolve<'a>(
        &'a self,
        scope: &'a EgressScope,
        host: &'a str,
        _port: Option<u16>,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            self.check(scope)?;
            if !host.is_ascii() || host.is_empty() || host.len() > 253 {
                return Err(refused());
            }
            let name = host.to_owned();
            let platform = self.host.clone();
            let revoked = self.revoked.clone();
            let owner = self.scope.native_owner()?;
            let addresses = tokio::task::spawn_blocking(move || {
                let _owner = owner;
                if !provider_current(platform.as_ref(), &revoked) {
                    return Err(refused());
                }
                let addresses = platform.resolve_name(&name);
                if !provider_current(platform.as_ref(), &revoked) {
                    return Err(refused());
                }
                let addresses = addresses?;
                if addresses.is_empty() || addresses.len() > 64 {
                    return Err(refused());
                }
                Ok(addresses)
            })
            .await
            .map_err(|_| {
                EngineError::Conflict("outbound resolver ended without a receipt".into())
            })??;
            self.check(scope)?;
            Ok(EgressResolution {
                scope: self.scope.clone(),
                addresses,
            })
        })
    }
}

fn provider_current(host: &dyn AndroidEgressHost, revoked: &LanRevocation) -> bool {
    if !revoked.is_revoked() && !host.current() {
        revoked.revoke();
    }
    !revoked.is_revoked()
}

fn refused() -> EngineError {
    EngineError::Unsupported("selected outbound Network refused the operation".into())
}

#[cfg(all(test, unix))]
#[path = "service_egress_android_tests.rs"]
mod tests;
