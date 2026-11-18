//! Same-process worker capability handoff. No guest/public JSON selects a Network
//! or grants permissions: actual native record flags and retained callbacks do.
use super::{BridgeError, ServiceRuntime, WorkerControl};
use std::sync::Arc;

#[uniffi::export(callback_interface)]
pub trait WorkerEgressPlatform: Send + Sync {
    fn current(&self) -> bool;
    fn read_capture(&self) -> String;
    fn install_revoker(&self, revoker: Arc<WorkerEgressRevoker>) -> bool;
    fn bind_socket(&self, descriptor: i32) -> bool;
    fn resolve_name(&self, ascii: String) -> Vec<String>;
}

#[derive(uniffi::Record)]
pub struct WorkerExecutionIdentity {
    pub instance_id: String,
    pub generation: String,
}

#[cfg(all(feature = "runtime", unix))]
struct ReplacementPreparation {
    ticket: wasmdroid_wasmer_local::service_egress_regrant::EgressRegrantTicket,
    selected: Arc<wasmdroid_wasmer_local::service_egress_android::AndroidSelectedNetwork>,
    policy: Arc<runtime::Policy>,
    limits: wasmdroid_wasmer_local::service_egress::EgressLimits,
}
/// Caller owns this handle before native installation can allocate a manager.
/// Errors/cancelled observers do not discard actual candidate cleanup receipts.
#[derive(uniffi::Object)]
pub struct WorkerEgressReplacement {
    #[cfg(all(feature = "runtime", unix))]
    inner: Arc<wasmdroid_wasmer_local::service_driver::ServiceDriver>,
    #[cfg(all(feature = "runtime", unix))]
    control: wasmdroid_wasmer_local::service_egress_regrant::EgressRegrantControl,
    #[cfg(all(feature = "runtime", unix))]
    pending: std::sync::Mutex<Option<ReplacementPreparation>>,
}
#[uniffi::export]
impl WorkerEgressReplacement {
    pub fn dispatch(&self) -> Result<(), BridgeError> {
        #[cfg(all(feature = "runtime", unix))]
        {
            let prepared = self
                .pending
                .lock()
                .map_err(|_| private_error())?
                .take()
                .ok_or_else(private_error)?;
            self.inner.install_running_egress(
                prepared.ticket,
                prepared.selected,
                prepared.policy,
                prepared.limits,
            )?;
            Ok(())
        }
        #[cfg(not(all(feature = "runtime", unix)))]
        {
            Err(private_error())
        }
    }
    pub fn cancel(&self) {
        #[cfg(all(feature = "runtime", unix))]
        {
            self.control.cancel();
            // Never hold the facade mutex through ticket Drop/driver callbacks.
            let prepared = self
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.take());
            drop(prepared);
        }
    }
    pub fn status(&self) -> Result<String, BridgeError> {
        #[cfg(all(feature = "runtime", unix))]
        {
            Ok(serde_json::to_string(&self.control.status()?)?)
        }
        #[cfg(not(all(feature = "runtime", unix)))]
        {
            Err(private_error())
        }
    }
}

/// Weak handle avoids callback -> revoker -> selected -> callback ownership cycles.
/// It is installed before long native preparation, independently of READY.
#[derive(uniffi::Object)]
pub struct WorkerEgressRevoker {
    #[cfg(all(feature = "runtime", unix))]
    selected:
        std::sync::Weak<wasmdroid_wasmer_local::service_egress_android::AndroidSelectedNetwork>,
}
#[uniffi::export]
impl WorkerEgressRevoker {
    pub fn revoke(&self) {
        #[cfg(all(feature = "runtime", unix))]
        if let Some(selected) = self.selected.upgrade() {
            selected.revoke();
        }
    }
}

#[cfg(any(all(feature = "runtime", unix), test))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureFrame {
    network_epoch: u64,
    own_addresses: Vec<String>,
    on_link: Vec<String>,
    nat64: Vec<String>,
}
fn private_error() -> BridgeError {
    BridgeError::Operation {
        reason: "Private outbound network capture is unavailable".into(),
    }
}
#[cfg(any(all(feature = "runtime", unix), test))]
fn capture_frame(
    raw: &str,
) -> Result<(u64, wasmdroid_engine::network::egress::EgressContext), BridgeError> {
    if raw.len() > 16 * 1024 {
        return Err(private_error());
    }
    let frame: CaptureFrame = serde_json::from_str(raw).map_err(|_| private_error())?;
    if frame.network_epoch == 0
        || frame.network_epoch > i64::MAX as u64
        || frame.own_addresses.is_empty()
        || frame.own_addresses.len() > 128
        || frame.on_link.len() > 128
        || frame.nat64.len() > 8
        || frame.own_addresses.iter().any(|value| value.len() > 64)
        || frame
            .on_link
            .iter()
            .chain(&frame.nat64)
            .any(|value| value.len() > 72)
    {
        return Err(private_error());
    }
    use wasmdroid_engine::network::egress::{EgressContext, NetworkPrefix};
    let own = frame
        .own_addresses
        .iter()
        .map(|value| {
            value
                .parse::<std::net::IpAddr>()
                .map_err(|_| private_error())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let routes = frame
        .on_link
        .iter()
        .map(|value| NetworkPrefix::parse(value).map_err(|_| private_error()))
        .collect::<Result<Vec<_>, _>>()?;
    let nat64 = frame
        .nat64
        .iter()
        .map(|value| NetworkPrefix::parse(value).map_err(|_| private_error()))
        .collect::<Result<Vec<_>, _>>()?;
    let context = EgressContext::new(own, routes, nat64).map_err(|_| private_error())?;
    Ok((frame.network_epoch, context))
}

#[cfg(all(feature = "runtime", unix))]
mod runtime {
    use super::*;
    use std::{
        net::{IpAddr, SocketAddr},
        sync::{Mutex, mpsc},
        thread,
    };
    use wasmdroid_engine::{EngineError, Result, network::egress::EgressPolicy};
    use wasmdroid_wasmer_local::{
        service_egress::{DestinationPolicy, EgressScope},
        service_egress_android::AndroidEgressHost,
    };
    type Callback = Box<dyn WorkerEgressPlatform>;
    static CALLBACK_RELEASE: Mutex<Option<mpsc::Sender<Callback>>> = Mutex::new(None);

    pub(super) struct Platform {
        callback: Option<Callback>,
        release: mpsc::Sender<Callback>,
    }

    impl Platform {
        pub(super) fn new(callback: Callback) -> std::result::Result<Arc<Self>, BridgeError> {
            let mut release = CALLBACK_RELEASE.lock().map_err(|_| private_error())?;
            let release = if let Some(sender) = release.as_ref() {
                sender.clone()
            } else {
                let (sender, receiver) = mpsc::channel::<Callback>();
                thread::Builder::new()
                    .name("wasmdroid-egress-release".into())
                    .spawn(move || {
                        while let Ok(callback) = receiver.recv() {
                            drop(callback);
                        }
                    })
                    .map_err(|_| private_error())?;
                *release = Some(sender.clone());
                sender
            };
            Ok(Arc::new(Self {
                callback: Some(callback),
                release,
            }))
        }

        pub(super) fn callback(&self) -> &dyn WorkerEgressPlatform {
            self.callback.as_deref().expect("live egress callback")
        }
    }

    impl Drop for Platform {
        fn drop(&mut self) {
            // UniFFI frees a foreign object by calling back into Java. A guest
            // fiber can release the last reference to a drained capture.
            if let Some(callback) = self.callback.take()
                && let Err(disconnected) = self.release.send(callback)
            {
                // A failed release thread cannot handle this drop safely.
                // Keep the handle until the worker process exits.
                std::mem::forget(disconnected.0);
            }
        }
    }

    impl AndroidEgressHost for Platform {
        fn current(&self) -> bool {
            self.callback().current()
        }
        fn bind_socket(&self, descriptor: i32) -> bool {
            self.callback().bind_socket(descriptor)
        }
        fn resolve_name(&self, ascii: &str) -> Result<Vec<IpAddr>> {
            let values = self.callback().resolve_name(ascii.into());
            if values.is_empty() || values.len() > 64 {
                return Err(EngineError::Conflict("network DNS is unavailable".into()));
            }
            values
                .iter()
                .map(|value| {
                    if value.len() > 64 {
                        return Err(EngineError::Conflict("invalid network DNS answer".into()));
                    }
                    value
                        .parse()
                        .map_err(|_| EngineError::Conflict("invalid network DNS answer".into()))
                })
                .collect()
        }
    }
    pub(super) struct Policy {
        pub scope: EgressScope,
        pub policy: EgressPolicy,
    }
    impl std::fmt::Debug for Policy {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("CapturedEgressPolicy(private)")
        }
    }
    impl Policy {
        fn check(&self, scope: &EgressScope) -> Result<()> {
            if scope == &self.scope {
                Ok(())
            } else {
                Err(EngineError::Conflict("outbound scope changed".into()))
            }
        }
    }
    impl DestinationPolicy for Policy {
        fn authorize_name(&self, scope: &EgressScope, _host: &str) -> Result<()> {
            self.check(scope)?;
            self.policy.authorize_dns()
        }
        fn authorize_ip(&self, scope: &EgressScope, ip: IpAddr) -> Result<()> {
            self.check(scope)?;
            self.policy.authorize_ip(ip)
        }
        fn authorize_tcp(&self, scope: &EgressScope, peer: SocketAddr) -> Result<()> {
            self.check(scope)?;
            self.policy.authorize_tcp(peer)
        }
    }
}

#[uniffi::export]
impl ServiceRuntime {
    pub fn egress_attachment_status(&self) -> Result<String, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            Ok(serde_json::to_string(&self.inner.egress_status()?)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(private_error())
        }
    }
    /// This call creates no manager. A returned error proves no admission; an
    /// actual installation is dispatched only after the caller owns its handle.
    pub fn begin_egress_replacement(
        &self,
        platform: Box<dyn WorkerEgressPlatform>,
    ) -> Result<Arc<WorkerEgressReplacement>, BridgeError> {
        #[cfg(all(feature = "runtime", unix))]
        {
            use wasmdroid_wasmer_local::{
                service_egress::EgressLimits, service_egress_android::AndroidSelectedNetwork,
            };
            let platform = runtime::Platform::new(platform)?;
            if !platform.callback().current() {
                return Err(private_error());
            }
            let (epoch, context) = capture_frame(&platform.callback().read_capture())?;
            let ticket = self.inner.reserve_running_egress(epoch)?;
            let control = ticket.control();
            let scope = ticket.scope().clone();
            let internet = ticket.internet();
            let lan = ticket.lan();
            let selected = AndroidSelectedNetwork::new(scope.clone(), platform.clone())?;
            if !platform
                .callback()
                .install_revoker(Arc::new(WorkerEgressRevoker {
                    selected: Arc::downgrade(&selected),
                }))
                || !platform.callback().current()
            {
                selected.revoke();
                return Err(private_error());
            }
            let policy = Arc::new(runtime::Policy {
                scope,
                policy: wasmdroid_engine::network::egress::EgressPolicy::new(
                    internet, lan, context,
                ),
            });
            let limits = EgressLimits::new(
                16,
                4,
                32,
                std::time::Duration::from_secs(15),
                std::time::Duration::from_secs(10),
            )?;
            Ok(Arc::new(WorkerEgressReplacement {
                inner: self.inner.clone(),
                control,
                pending: std::sync::Mutex::new(Some(ReplacementPreparation {
                    ticket,
                    selected,
                    policy,
                    limits,
                })),
            }))
        }
        #[cfg(not(all(feature = "runtime", unix)))]
        {
            let _ = platform;
            Err(private_error())
        }
    }
    /// Production worker borrows the descriptor. A lowering failure before Rust
    /// entry leaves the original ParcelFileDescriptor owned by its Java caller.
    #[uniffi::constructor]
    pub fn start_borrowed(
        data_dir: String,
        owner_descriptor: i32,
        instance_id: String,
        generation: String,
        spec_json: String,
        worker_control: Box<dyn WorkerControl>,
        network_config: Option<String>,
    ) -> Result<Arc<Self>, BridgeError> {
        #[cfg(unix)]
        {
            use std::os::fd::{BorrowedFd, IntoRawFd};
            if owner_descriptor < 0 {
                return Err(private_error());
            }
            // SAFETY: same-process worker keeps its original PFD alive until
            // this synchronous call returns. Only the duplicate is consumed.
            let owner = unsafe { BorrowedFd::borrow_raw(owner_descriptor) }
                .try_clone_to_owned()
                .map_err(|_| private_error())?;
            Self::start(
                data_dir,
                owner.into_raw_fd(),
                instance_id,
                generation,
                spec_json,
                worker_control,
                network_config,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (
                data_dir,
                owner_descriptor,
                instance_id,
                generation,
                spec_json,
                worker_control,
                network_config,
            );
            Err(private_error())
        }
    }
    #[uniffi::constructor]
    pub fn start_with_egress(
        data_dir: String,
        owner_descriptor: i32,
        identity: WorkerExecutionIdentity,
        spec_json: String,
        worker_control: Box<dyn WorkerControl>,
        network_config: Option<String>,
        platform: Box<dyn WorkerEgressPlatform>,
    ) -> Result<Arc<Self>, BridgeError> {
        let WorkerExecutionIdentity {
            instance_id,
            generation,
        } = identity;
        #[cfg(unix)]
        let owner = {
            use std::os::fd::BorrowedFd;
            if owner_descriptor < 0 {
                return Err(private_error());
            }
            // SAFETY: the private same-process worker borrows its still-owned
            // PFD for this synchronous call. Rust owns only this new duplicate.
            std::fs::File::from(
                unsafe { BorrowedFd::borrow_raw(owner_descriptor) }
                    .try_clone_to_owned()
                    .map_err(|_| private_error())?,
            )
        };
        #[cfg(all(feature = "runtime", unix))]
        {
            use wasmdroid_engine::{api::InstanceSpec, driver::RuntimeDriver};
            use wasmdroid_wasmer_local::{
                service_driver::ServiceDriver, service_egress::EgressLimits,
                service_egress_android::AndroidSelectedNetwork,
            };
            if spec_json.len() > 128 * 1024
                || network_config
                    .as_ref()
                    .is_some_and(|value| value.len() > 32 * 1024)
            {
                return Err(private_error());
            }
            let spec: InstanceSpec = serde_json::from_str(&spec_json)?;
            let session = wasmdroid_engine::session::EngineSession::join_worker(data_dir, owner)?;
            let config = network_config
                .map(|raw| serde_json::from_str(&raw).map_err(|_| private_error()))
                .transpose()?;
            let inner = ServiceDriver::with_network(
                session,
                instance_id.clone(),
                generation,
                Arc::new(super::PlatformControl(worker_control)),
                config,
            );
            inner.create_with_egress_spec(&instance_id, &spec)?;
            let platform = runtime::Platform::new(platform)?;
            if !platform.callback().current() {
                return Err(private_error());
            }
            let (epoch, context) = capture_frame(&platform.callback().read_capture())?;
            let scope = inner.capture_egress_scope(1, epoch)?;
            let internet = scope.internet()?;
            let lan = scope.lan()?;
            let selected = AndroidSelectedNetwork::new(scope.clone(), platform.clone())?;
            let revoker = Arc::new(WorkerEgressRevoker {
                selected: Arc::downgrade(&selected),
            });
            if !platform.callback().install_revoker(revoker) || !platform.callback().current() {
                selected.revoke();
                return Err(private_error());
            }
            let policy = Arc::new(runtime::Policy {
                scope,
                policy: wasmdroid_engine::network::egress::EgressPolicy::new(
                    internet, lan, context,
                ),
            });
            let limits = EgressLimits::new(
                16,
                4,
                32,
                std::time::Duration::from_secs(15),
                std::time::Duration::from_secs(10),
            )?;
            inner.prepare_egress(selected, policy, limits)?;
            inner.start(&instance_id)?;
            Ok(Arc::new(Self {
                inner: Arc::new(inner),
            }))
        }
        #[cfg(not(all(feature = "runtime", unix)))]
        {
            #[cfg(unix)]
            drop(owner);
            let _ = (
                data_dir,
                owner_descriptor,
                instance_id,
                generation,
                spec_json,
                worker_control,
                network_config,
                platform,
            );
            Err(private_error())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_requires_exact_positive_bounded_epoch_and_numeric_shape() {
        let valid = serde_json::json!({"network_epoch":1,"own_addresses":["192.168.1.2"],"on_link":["192.168.1.0/24"],"nat64":[]});
        assert_eq!(capture_frame(&valid.to_string()).unwrap().0, 1);
        for value in [
            serde_json::json!(0),
            serde_json::json!("1"),
            serde_json::json!(u64::MAX),
            serde_json::Value::Null,
        ] {
            let mut bad = valid.clone();
            bad["network_epoch"] = value;
            assert!(capture_frame(&bad.to_string()).is_err());
        }
        assert!(capture_frame(&format!("{}{{}}", valid)).is_err());
        let mut bad = valid.clone();
        bad["network_handle"] = 1.into();
        assert!(capture_frame(&bad.to_string()).is_err());
        let mut bad = valid.clone();
        bad["own_addresses"] = serde_json::json!([]);
        assert!(capture_frame(&bad.to_string()).is_err());
        let mut bad = valid.clone();
        bad["own_addresses"] = serde_json::json!(["PRIVATE.example"]);
        assert!(capture_frame(&bad.to_string()).is_err());
        let mut bad = valid.clone();
        bad["on_link"] = serde_json::json!(["0.0.0.0/0"]);
        assert!(capture_frame(&bad.to_string()).is_err());
        let mut bad = valid;
        bad["nat64"] = serde_json::json!(["64:ff9b::/95"]);
        assert!(capture_frame(&bad.to_string()).is_err());
    }
}
