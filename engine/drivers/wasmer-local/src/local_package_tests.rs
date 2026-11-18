use super::*;
use std::io::Write;
use wasmdroid_engine::{
    local_import::LocalImport,
    session::EngineSession,
    store::{ContentStore, InstallBudget},
};

fn module() -> Vec<u8> {
    wasmer::wat2wasm(include_bytes!("../../../../fixtures/hello-wasi.wat"))
        .unwrap()
        .into_owned()
}
fn inspect_bytes(bytes: &[u8]) -> wasmdroid_engine::Result<Inspection> {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    LocalInspector.inspect(file.as_file(), &OperationControl::default())
}
fn webc(wasm: &[u8], bad_signature: bool) -> Vec<u8> {
    use webc::{
        metadata::{Atom, AtomSignature, Command, Manifest},
        v3::{
            SignatureAlgorithm, Timestamps,
            write::{FileEntry, Writer},
        },
    };
    let mut manifest = Manifest::default();
    manifest.entrypoint = Some("main".into());
    let signed_bytes = if bad_signature {
        b"different code".as_slice()
    } else {
        wasm
    };
    let hash = wasmer_types::ModuleHash::new(signed_bytes);
    manifest.atoms.insert(
        "main".into(),
        Atom {
            kind: "https://webc.org/kind/wasm".parse().unwrap(),
            signature: AtomSignature::Sha256(hash.as_bytes().try_into().unwrap()).to_string(),
            annotations: Default::default(),
        },
    );
    let mut command = Command {
        runner: "https://webc.org/runner/wasi".into(),
        annotations: Default::default(),
    };
    command.annotations.insert(
        "atom".into(),
        serde_json::from_value(serde_json::json!({"name":"main"})).unwrap(),
    );
    manifest.commands.insert("main".into(), command);
    Writer::default()
        .write_manifest(&manifest)
        .unwrap()
        .write_atoms(BTreeMap::from([(
            "main".parse().unwrap(),
            FileEntry::borrowed(wasm, Timestamps::default()),
        )]))
        .unwrap()
        .finish(SignatureAlgorithm::None)
        .unwrap()
        .to_vec()
}
#[test]
fn actual_raw_wasm_and_webc_use_the_same_verified_offline_execution_loader() {
    for bytes in [module(), webc(&module(), false)] {
        let dir = tempfile::tempdir().unwrap();
        let session = EngineSession::open(dir.path()).unwrap();
        let mut import = LocalImport::begin(
            session.connection().unwrap(),
            InstallBudget {
                max_new_bytes: bytes.len() as u64 * 2,
                max_store_bytes: None,
            },
            Some(bytes.len() as u64),
            Default::default(),
        )
        .unwrap();
        for chunk in bytes.chunks(64 * 1024) {
            import.append(chunk).unwrap();
        }
        let token = import
            .preview(&LocalInspector)
            .unwrap()
            .token
            .as_str()
            .to_owned();
        let artifact = import.commit(&token).unwrap();
        let store = ContentStore::open(session.root().join("store")).unwrap();
        let result =
            crate::webc_execution::run_offline(&store, &artifact, &[artifact.clone()], "main", &[])
                .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello from WasmDroid!\n");
    }
}
#[test]
fn malformed_native_component_and_invalid_start_modules_never_reach_compilation() {
    let valid = module();
    for bytes in [
        b"\x7fELF".to_vec(),
        b"\0asm\x0d\0\x01\0".to_vec(),
        valid[..valid.len() - 1].to_vec(),
        wasmer::wat2wasm(b"(module (func (export \"_start\") (param i32)))")
            .unwrap()
            .into_owned(),
        wasmer::wat2wasm(b"(module (func (export \"other\")))")
            .unwrap()
            .into_owned(),
    ] {
        assert!(inspect_bytes(&bytes).is_err());
    }
    assert!(inspect_bytes(&valid).is_ok());
    assert!(inspect_bytes(&webc(&valid, true)).is_err());
}
#[test]
fn metadata_preflight_rejects_deep_cbor_and_index_payload_redirection() {
    let control = OperationControl::default();
    let mut deep = vec![0x81; 34];
    deep.push(0);
    assert!(bounded_cbor(&deep, &control).is_err());
    assert!(bounded_cbor(&[0x9f, 1, 0xff], &control).is_ok());
    assert!(bounded_cbor(&[0xbf, 1, 0xff], &control).is_err());
    let mut bytes = webc(&module(), false);
    // Replace a valid manifest tag at its actual framed boundary. The index
    // must not be allowed to turn an atoms payload into unchecked manifest CBOR.
    let index_len = u64_le(&bytes, 9).unwrap();
    let manifest_offset = 8 + 9 + index_len;
    assert_eq!(bytes[manifest_offset], Tag::Manifest as u8);
    bytes[manifest_offset] = Tag::Atoms as u8;
    assert!(inspect_bytes(&bytes).is_err());
    let mut legacy = webc(&module(), false);
    legacy[7] = b'2';
    assert!(inspect_bytes(&legacy).is_err());
}
#[test]
fn cancellation_is_preserved_as_a_typed_engine_result() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&module()).unwrap();
    let control = OperationControl::default();
    assert!(control.cancel());
    assert!(matches!(
        LocalInspector.inspect(file.as_file(), &control),
        Err(EngineError::Cancelled)
    ));
}
