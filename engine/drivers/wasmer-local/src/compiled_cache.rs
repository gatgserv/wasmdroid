//! Wasmer's deserializer accepts native executable code. Its only disk input
//! here is an authenticated buffer from our private, locally-produced cache.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use wasmdroid_engine::{compiled_cache::TrustedCodeCache, session::EngineSession};
use wasmer::{Engine, Module, sys::NativeEngineExt};
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    PluggableRuntime,
    runtime::module_cache::{CacheError, ModuleCache, SharedCache},
};

#[derive(Debug, Default)]
pub(crate) struct CacheCounters {
    hits: AtomicU64,
    misses: AtomicU64,
    saves: AtomicU64,
    failures: AtomicU64,
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheActivity {
    pub disk_hits: u64,
    pub disk_misses: u64,
    pub disk_saves: u64,
    pub failures: u64,
}
impl CacheCounters {
    pub fn snapshot(&self) -> CacheActivity {
        CacheActivity {
            disk_hits: self.hits.load(Ordering::Relaxed),
            disk_misses: self.misses.load(Ordering::Relaxed),
            disk_saves: self.saves.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
        }
    }
}
#[derive(Debug)]
struct CompiledCache {
    disk: TrustedCodeCache,
    counters: Arc<CacheCounters>,
}
pub(crate) fn configure(
    runtime: &mut PluggableRuntime,
    session: &EngineSession,
) -> Arc<CacheCounters> {
    let counters = Arc::new(CacheCounters::default());
    match TrustedCodeCache::open(session) {
        Ok(disk) => {
            runtime.set_module_cache(SharedCache::default().with_fallback(CompiledCache {
                disk,
                counters: counters.clone(),
            }));
        }
        // Cache availability is optional. No failed cache operation grants new
        // permissions or enables unverified serialized-module loading.
        Err(_) => {
            counters.failures.fetch_add(1, Ordering::Relaxed);
        }
    }
    counters
}
fn context(key: ModuleHash, engine: &Engine) -> Vec<u8> {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    format!(
        "wasmdroid-native-cache-v1\n{}\n{}\n{}\n{:?}\n{:?}\n{page_size}\n{key}",
        env!("WASMDROID_CACHE_BUILD"),
        engine.deterministic_id(),
        engine.artifact_format(),
        engine.target(),
        engine.as_sys().inner().features()
    )
    .into_bytes()
}
#[async_trait::async_trait]
impl ModuleCache for CompiledCache {
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        let bytes = match self.disk.load(&context(key, engine)) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                self.counters.misses.fetch_add(1, Ordering::Relaxed);
                return Err(CacheError::NotFound);
            }
            Err(_) => {
                self.counters.failures.fetch_add(1, Ordering::Relaxed);
                return Err(CacheError::NotFound);
            }
        };
        // SAFETY: bytes are the exact HMAC-verified buffer written by this
        // application's compiler. Authentication covers both payload hash and
        // full source/runtime/compiler/CPU/build identity. Guests, registry
        // downloads and document imports cannot write this cache or its key.
        match unsafe { Module::deserialize(engine, bytes) } {
            Ok(module) => {
                self.counters.hits.fetch_add(1, Ordering::Relaxed);
                Ok(module)
            }
            Err(error) => {
                self.counters.failures.fetch_add(1, Ordering::Relaxed);
                Err(error.into())
            }
        }
    }
    async fn contains(&self, key: ModuleHash, engine: &Engine) -> Result<bool, CacheError> {
        Ok(self
            .disk
            .load(&context(key, engine))
            .ok()
            .flatten()
            .is_some())
    }
    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        let bytes = module.serialize()?;
        if self.disk.save_local(&context(key, engine), &bytes).is_err() {
            self.counters.failures.fetch_add(1, Ordering::Relaxed);
        } else {
            self.counters.saves.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}
