//! Stage 0 finite WASI probe; intentionally not a lifecycle driver yet.
//! Host filesystem and host networking are not enabled.
use std::{io::Read, sync::Arc};
use wasmer::{Module, Store};
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    Pipe, PluggableRuntime,
    runners::wasi::{RuntimeOrEngine, WasiRunner},
    runtime::task_manager::tokio::TokioTaskManager,
};

pub fn run_probe() -> anyhow::Result<String> {
    let executor = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _guard = executor.enter();
    let tasks = TokioTaskManager::new(executor.handle().clone());
    let runtime = PluggableRuntime::new(Arc::new(tasks));
    let store = Store::default();
    let wasm = wasmer::wat2wasm(include_bytes!("../../../../fixtures/hello-wasi.wat"))?;
    let module = Module::new(&store, &wasm)?;
    let (stdout, mut output) = Pipe::channel();
    {
        let mut runner = WasiRunner::new();
        runner.with_stdout(Box::new(stdout));
        runner.run_wasm(
            RuntimeOrEngine::Runtime(Arc::new(runtime)),
            "wasmdroid-probe",
            module,
            ModuleHash::new(&wasm),
        )?;
    }
    let mut text = String::new();
    output.read_to_string(&mut text)?;
    anyhow::ensure!(
        text == "Hello from WasmDroid!\n",
        "Unexpected WASI output: {text:?}"
    );
    Ok(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn executes_wasi_fd_write() {
        assert_eq!(super::run_probe().unwrap(), "Hello from WasmDroid!\n");
    }
}
