//! Count execution work until its callbacks actually return. A WASIX process
//! status alone is not proof that its threads have unwound.
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use wasmer::{Memory, StoreMut};
use wasmer_wasix::{
    os::task::thread::WasiThreadError,
    runtime::task_manager::{SpawnType, TaskWasm, VirtualTaskManager, tokio::TokioTaskManager},
};

#[derive(Debug)]
pub(crate) struct TrackedTasks {
    inner: TokioTaskManager,
    active: Arc<AtomicUsize>,
    #[cfg(unix)]
    interrupts: Arc<Interrupts>,
}

#[cfg(unix)]
#[derive(Default)]
struct Interrupts {
    next: std::sync::atomic::AtomicU64,
    stores: std::sync::Mutex<std::collections::BTreeMap<u64, InterruptEntry>>,
}
#[cfg(unix)]
struct InterruptEntry {
    handle: wasmer::Interrupter,
    signal_mask: String,
}
#[cfg(unix)]
impl std::fmt::Debug for Interrupts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interrupts").finish_non_exhaustive()
    }
}
#[cfg(unix)]
struct RegisteredStore {
    id: u64,
    registry: Arc<Interrupts>,
}

/// Android's parent runtime can block SIGUSR1. Wasmer uses that signal for
/// store interruption, so unblock it only on our dedicated execution worker and
/// restore the inherited mask when its callback leaves (including unwinding).
#[cfg(unix)]
struct SignalMaskGuard {
    previous: libc::sigset_t,
}
#[cfg(unix)]
impl SignalMaskGuard {
    fn enter() -> std::io::Result<Self> {
        let mut signals = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
        let mut previous = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
        // SAFETY: these are valid writable sigset_t allocations; sigemptyset
        // initializes the first, and successful pthread_sigmask initializes the
        // second. All calls affect only the current dedicated worker thread.
        unsafe {
            if libc::sigemptyset(signals.as_mut_ptr()) != 0
                || libc::sigaddset(signals.as_mut_ptr(), libc::SIGUSR1) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            let code =
                libc::pthread_sigmask(libc::SIG_UNBLOCK, signals.as_ptr(), previous.as_mut_ptr());
            if code != 0 {
                return Err(std::io::Error::from_raw_os_error(code));
            }
            Ok(Self {
                previous: previous.assume_init(),
            })
        }
    }
}
#[cfg(unix)]
impl Drop for SignalMaskGuard {
    fn drop(&mut self) {
        // SAFETY: previous was initialized by pthread_sigmask on this same
        // worker; this restores its exact prior mask without changing handlers.
        unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, std::ptr::null_mut());
        }
    }
}
#[cfg(unix)]
impl Drop for RegisteredStore {
    fn drop(&mut self) {
        self.registry
            .stores
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.id);
    }
}

struct Activity(Arc<AtomicUsize>);
impl Drop for Activity {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl TrackedTasks {
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            inner: TokioTaskManager::new(handle),
            active: Arc::new(AtomicUsize::new(0)),
            #[cfg(unix)]
            interrupts: Arc::new(Interrupts::default()),
        }
    }
    fn activity(&self) -> Activity {
        self.active.fetch_add(1, Ordering::AcqRel);
        Activity(self.active.clone())
    }
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// Best effort: callers must retry until actual callback activity reaches
    /// zero. A store may be between registration and entering machine code.
    pub fn interrupt_all(&self) -> usize {
        #[cfg(unix)]
        {
            let stores = self
                .interrupts
                .stores
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for entry in stores.values() {
                entry.handle.interrupt();
            }
            stores.len()
        }
        #[cfg(not(unix))]
        {
            0
        }
    }
    pub fn interrupt_diagnostics(&self) -> Vec<String> {
        #[cfg(unix)]
        {
            self.interrupts
                .stores
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .map(|entry| entry.signal_mask.clone())
                .collect()
        }
        #[cfg(not(unix))]
        {
            Vec::new()
        }
    }
}

impl VirtualTaskManager for TrackedTasks {
    fn build_memory(
        &self,
        store: &mut StoreMut,
        spawn_type: SpawnType,
    ) -> Result<Option<Memory>, WasiThreadError> {
        self.inner.build_memory(store, spawn_type)
    }
    fn sleep_now(
        &self,
        time: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>> {
        self.inner.sleep_now(time)
    }
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        let activity = self.activity();
        self.inner.task_shared(Box::new(move || {
            Box::pin(async move {
                let _activity = activity;
                task().await;
            })
        }))
    }
    fn task_wasm(&self, mut task: TaskWasm) -> Result<(), WasiThreadError> {
        let activity = self.activity();
        let run = task.callbacks.run;
        #[cfg(unix)]
        let registry = self.interrupts.clone();
        task.callbacks.run = Box::new(move |properties| {
            let _activity = activity;
            #[cfg(unix)]
            let _signal_mask = match SignalMaskGuard::enter() {
                Ok(guard) => guard,
                Err(_) => {
                    // No guest callback has entered machine code yet.
                    properties
                        .ctx
                        .data(&properties.store)
                        .process
                        .signal_process(wasmer_wasix_types::wasi::Signal::Sigkill);
                    properties
                        .ctx
                        .data(&properties.store)
                        .thread
                        .set_status_finished(Err(wasmer::RuntimeError::new(
                            "Worker interrupt signal could not be unblocked",
                        )
                        .into()));
                    return;
                }
            };
            #[cfg(unix)]
            let _registration = {
                let id = registry.next.fetch_add(1, Ordering::Relaxed);
                registry
                    .stores
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(
                        id,
                        InterruptEntry {
                            handle: properties.store.interrupter(),
                            signal_mask: std::fs::read_to_string("/proc/thread-self/status")
                                .ok()
                                .and_then(|status| {
                                    status.lines().find_map(|line| {
                                        line.strip_prefix("SigBlk:")
                                            .map(|value| value.trim().to_owned())
                                    })
                                })
                                .unwrap_or_else(|| "unavailable".into()),
                        },
                    );
                RegisteredStore { id, registry }
            };
            run(properties);
        });
        self.inner.task_wasm(task)
    }
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        let activity = self.activity();
        self.inner.task_dedicated(Box::new(move || {
            let _activity = activity;
            task();
        }))
    }
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Ok(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn interrupt_guard_unblocks_only_its_worker_and_restores_inherited_mask() {
        // A fresh test thread owns every mask change. No signal is sent here.
        std::thread::spawn(|| unsafe {
            let mut original = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
            assert_eq!(
                libc::pthread_sigmask(0, std::ptr::null(), original.as_mut_ptr()),
                0
            );
            let original = original.assume_init();
            let _restore = SignalMaskGuard { previous: original };
            let mut blocked = original;
            assert_eq!(libc::sigaddset(&mut blocked, libc::SIGUSR1), 0);
            assert_eq!(libc::sigaddset(&mut blocked, libc::SIGUSR2), 0);
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_SETMASK, &blocked, std::ptr::null_mut()),
                0
            );
            let current = || {
                let mut mask = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
                assert_eq!(
                    libc::pthread_sigmask(0, std::ptr::null(), mask.as_mut_ptr()),
                    0
                );
                mask.assume_init()
            };
            {
                let _guard = SignalMaskGuard::enter().unwrap();
                let mask = current();
                assert_eq!(libc::sigismember(&mask, libc::SIGUSR1), 0);
                assert_eq!(libc::sigismember(&mask, libc::SIGUSR2), 1);
            }
            let mask = current();
            assert_eq!(libc::sigismember(&mask, libc::SIGUSR1), 1);
            assert_eq!(libc::sigismember(&mask, libc::SIGUSR2), 1);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn task_remains_active_until_callback_returns() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let tasks = TrackedTasks::new(runtime.handle().clone());
        let (started, start) = std::sync::mpsc::channel();
        let (release, finish) = std::sync::mpsc::channel();
        tasks
            .task_dedicated(Box::new(move || {
                started.send(()).unwrap();
                finish.recv().unwrap();
            }))
            .unwrap();
        start.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(tasks.active(), 1);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while tasks.active() != 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(tasks.active(), 0);
    }
}
