//! Real descriptor/owner retention with controlled same-process callbacks.
//! These fixtures never attest Android Network routing, DNS or JNI behavior.
use super::*;
use std::{
    os::fd::AsRawFd,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
};
use tokio::sync::Notify;
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec},
    session::EngineSession,
    state::RunStartSource,
    store::Digest,
};

struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
    started: Notify,
    live: AtomicBool,
}
impl Gate {
    fn new(open: bool) -> Arc<Self> {
        Arc::new(Self {
            open: Mutex::new(open),
            changed: Condvar::new(),
            started: Notify::new(),
            live: AtomicBool::new(false),
        })
    }
    fn enter(&self) {
        self.live.store(true, Ordering::SeqCst);
        self.started.notify_one();
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
        self.live.store(false, Ordering::SeqCst);
    }
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct Host {
    current: AtomicBool,
    binder: Arc<Gate>,
    dns: Arc<Gate>,
    descriptor: AtomicI32,
    bound: bool,
}
impl AndroidEgressHost for Host {
    fn current(&self) -> bool {
        self.current.load(Ordering::SeqCst)
    }
    fn bind_socket(&self, descriptor: i32) -> bool {
        self.descriptor.store(descriptor, Ordering::SeqCst);
        self.binder.enter();
        // SAFETY: the adapter retains ownership throughout this callback.
        assert_ne!(unsafe { libc::fcntl(descriptor, libc::F_GETFD) }, -1);
        self.bound
    }
    fn resolve_name(&self, name: &str) -> Result<Vec<IpAddr>> {
        assert_eq!(name, "server.invalid");
        self.dns.enter();
        Ok(vec!["203.0.113.7".parse().unwrap()])
    }
}
fn host(binder: Arc<Gate>, dns: Arc<Gate>, bound: bool) -> Arc<Host> {
    Arc::new(Host {
        current: AtomicBool::new(true),
        binder,
        dns,
        descriptor: AtomicI32::new(-1),
        bound,
    })
}
fn fixture() -> (tempfile::TempDir, EngineSession, EgressScope) {
    let dir = tempfile::tempdir().unwrap();
    let session = EngineSession::open(dir.path()).unwrap();
    let reference = ArtifactReference::parse("wasmer.io/fixture/outbound@1.0.0").unwrap();
    session
        .state()
        .put_artifact(&Artifact {
            reference: reference.clone(),
            digest: Digest::of(b"metadata fixture"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: Default::default(),
        })
        .unwrap();
    let spec: InstanceSpec = serde_json::from_value(
        serde_json::json!({"artifact_ref":reference,"command":"run","lan_access":true}),
    )
    .unwrap();
    let record = session.state().create_instance("owned", &spec).unwrap();
    session
        .state()
        .begin_authorized_run(
            "owned",
            record.version,
            0,
            "generation",
            Some(1),
            RunStartSource::Manual,
        )
        .unwrap();
    let scope = EgressScope::capture(&session, "owned", "generation", &spec, 1, 1).unwrap();
    (dir, session, scope)
}
async fn reopen_after_actual_return(path: &std::path::Path) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            match EngineSession::open(path) {
                Ok(session) => {
                    drop(session);
                    break;
                }
                Err(EngineError::Conflict(_)) => tokio::task::yield_now().await,
                Err(error) => panic!("{error}"),
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn binder_receives_owned_duplicate_and_it_closes_only_after_actual_callback() {
    let (_dir, _session, scope) = fixture();
    let host = host(Gate::new(true), Gate::new(true), true);
    let selected = AndroidSelectedNetwork::new(scope.clone(), host.clone()).unwrap();
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    let original = socket.as_raw_fd();
    selected.bind(&scope, &socket).await.unwrap();
    let duplicate = host.descriptor.load(Ordering::SeqCst);
    assert_ne!(duplicate, original);
    // SAFETY: this is an observation only; it never adopts or closes the number.
    assert_eq!(unsafe { libc::fcntl(duplicate, libc::F_GETFD) }, -1);
    assert_ne!(unsafe { libc::fcntl(original, libc::F_GETFD) }, -1);
}

#[tokio::test]
async fn aborted_bind_observer_keeps_duplicate_and_native_owner_until_real_return() {
    let (dir, session, scope) = fixture();
    let gate = Gate::new(false);
    let _release = Release(gate.clone());
    let host = host(gate.clone(), Gate::new(true), true);
    let selected = AndroidSelectedNetwork::new(scope.clone(), host.clone()).unwrap();
    let observed = {
        let selected = selected.clone();
        let scope = scope.clone();
        tokio::spawn(async move {
            let socket = tokio::net::TcpSocket::new_v4().unwrap();
            selected.bind(&scope, &socket).await
        })
    };
    gate.started.notified().await;
    selected.revoke();
    observed.abort();
    let _ = observed.await;
    drop(selected);
    drop(scope);
    drop(session);
    assert!(gate.live.load(Ordering::SeqCst));
    assert!(matches!(
        EngineSession::open(dir.path()),
        Err(EngineError::Conflict(_))
    ));
    let duplicate = host.descriptor.load(Ordering::SeqCst);
    // SAFETY: observation of the callback-owned still-live descriptor only.
    assert_ne!(unsafe { libc::fcntl(duplicate, libc::F_GETFD) }, -1);
    gate.release();
    reopen_after_actual_return(dir.path()).await;
    assert!(!gate.live.load(Ordering::SeqCst));
    assert_eq!(unsafe { libc::fcntl(duplicate, libc::F_GETFD) }, -1);
}

#[tokio::test]
async fn abandoned_dns_observer_retains_owner_through_actual_provider_return() {
    let (dir, session, scope) = fixture();
    let gate = Gate::new(false);
    let _release = Release(gate.clone());
    let host = host(Gate::new(true), gate.clone(), true);
    let selected = AndroidSelectedNetwork::new(scope.clone(), host).unwrap();
    let observed = {
        let selected = selected.clone();
        let scope = scope.clone();
        tokio::spawn(async move { selected.resolve(&scope, "server.invalid", None).await })
    };
    gate.started.notified().await;
    selected.revoke();
    observed.abort();
    let _ = observed.await;
    drop(selected);
    drop(scope);
    drop(session);
    assert!(gate.live.load(Ordering::SeqCst));
    assert!(matches!(
        EngineSession::open(dir.path()),
        Err(EngineError::Conflict(_))
    ));
    gate.release();
    reopen_after_actual_return(dir.path()).await;
    assert!(!gate.live.load(Ordering::SeqCst));
}

#[tokio::test]
async fn late_success_after_capture_revoke_is_refused_after_real_bind_returns() {
    let (_dir, _session, scope) = fixture();
    let gate = Gate::new(false);
    let _release = Release(gate.clone());
    let host = host(gate.clone(), Gate::new(true), true);
    let selected = AndroidSelectedNetwork::new(scope.clone(), host).unwrap();
    let observed = {
        let selected = selected.clone();
        let scope = scope.clone();
        tokio::spawn(async move {
            let socket = tokio::net::TcpSocket::new_v4().unwrap();
            selected.bind(&scope, &socket).await
        })
    };
    gate.started.notified().await;
    selected.revoke();
    gate.release();
    assert!(observed.await.unwrap().is_err());
    assert!(!selected.current());
}

#[tokio::test]
async fn guest_checks_use_native_signal_and_provider_loss_revokes_capture() {
    let (_dir, _session, scope) = fixture();
    let callbacks = host(Gate::new(true), Gate::new(true), true);
    let selected = AndroidSelectedNetwork::new(scope.clone(), callbacks.clone()).unwrap();

    callbacks.current.store(false, Ordering::SeqCst);
    assert!(selected.current());
    assert!(selected.check(&scope).is_ok());
    assert!(
        selected
            .resolve(&scope, "server.invalid", None)
            .await
            .is_err()
    );
    assert!(!selected.current());
    assert!(selected.check(&scope).is_err());
}

#[test]
fn matching_fields_without_native_issuance_cannot_create_platform_capture() {
    let scope = EgressScope::new("owned".into(), "generation".into(), 0, 1, 1).unwrap();
    assert!(
        AndroidSelectedNetwork::new(scope, host(Gate::new(true), Gate::new(true), true)).is_err()
    );
}
