use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn sources(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).expect("cache fingerprint source directory") {
            sources(&entry.unwrap().path(), files);
        }
    } else if path.extension().is_some_and(|e| e == "rs") {
        files.push(path.to_owned());
    }
}
fn main() {
    let driver = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let root = driver.join("../../..").canonicalize().unwrap();
    let mut files = vec![
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("rust-toolchain.toml"),
        root.join("engine/Cargo.toml"),
        root.join("engine/ffi/Cargo.toml"),
        driver.join("Cargo.toml"),
        driver.join("build.rs"),
        driver.join("vendor/wasmer-wasix/Cargo.toml"),
        driver.join("vendor/virtual-net/Cargo.toml"),
    ];
    sources(&driver.join("src"), &mut files);
    sources(&driver.join("vendor/wasmer-wasix/src"), &mut files);
    sources(&driver.join("vendor/virtual-net/src"), &mut files);
    sources(&root.join("engine/src"), &mut files);
    files.sort();
    let mut digest = Sha256::new();
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&path).expect("cache fingerprint source file");
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    let rustc = Command::new(std::env::var_os("RUSTC").unwrap())
        .arg("-vV")
        .output()
        .expect("rustc version");
    assert!(rustc.status.success());
    digest.update(rustc.stdout);
    for name in [
        "TARGET",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_FEATURE_CRANELIFT",
        "CARGO_FEATURE_SINGLEPASS",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        digest.update(name.as_bytes());
        digest.update(std::env::var(name).unwrap_or_default().as_bytes());
        digest.update([0]);
    }
    println!(
        "cargo:rustc-env=WASMDROID_CACHE_BUILD={:x}",
        digest.finalize()
    );
}
