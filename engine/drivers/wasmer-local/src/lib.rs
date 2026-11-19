//! Wasmer backend: supervised package execution and finite acceptance probes.
//! Filesystem/network access uses explicit virtual capabilities, never ambient
//! host directories, terminal descriptors or unrestricted host networking.
#[cfg(not(any(feature = "cranelift", feature = "singlepass")))]
compile_error!("Select the cranelift or singlepass driver feature");
pub mod benchmark;
pub mod cancellation;
mod compiled_cache;
pub mod http_execution;
pub mod local_package;
mod loopback;
mod service_broker;
pub mod service_driver;
pub mod service_egress;
pub mod service_egress_android;
pub mod service_egress_regrant;
pub mod service_execution;
mod service_input;
pub mod service_lan;
mod service_network;
mod service_startup_port;
mod service_stdio;
pub use service_input::InputControl;
pub use service_stdio::TerminalHandle;
mod tasks;
mod volume_fs;
pub mod webc_execution;
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
