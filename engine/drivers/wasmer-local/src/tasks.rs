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
        }
    }
    fn activity(&self) -> Activity {
        self.active.fetch_add(1, Ordering::AcqRel);
        Activity(self.active.clone())
    }
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
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
        task.callbacks.run = Box::new(move |properties| {
            let _activity = activity;
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
