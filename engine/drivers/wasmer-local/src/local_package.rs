//! Inspection never compiles code, executes guests or opens manifest resources.
use anyhow::{Context, Result, ensure};
use shared_buffer::OwnedBuffer;
use std::{collections::BTreeMap, fs::File, str::FromStr};
use wasmdroid_engine::{
    EngineError,
    api::ArtifactKind,
    local_import::{Dependency, Inspection, Inspector},
    operation::OperationControl,
    store::Digest,
};
use wasmer_config::package::{PackageHash, PackageId, PackageSource};
use wasmer_wasix::{
    bin_factory::{BinaryPackage, BinaryPackageCommand},
    runtime::resolver::PackageInfo,
};
use webc::{
    Container,
    v3::{Index, Span, Tag, read::Section},
};
#[cfg(test)]
#[path = "local_package_tests.rs"]
mod tests;

pub struct LocalInspector;
impl Inspector for LocalInspector {
    fn inspect(
        &self,
        file: &File,
        control: &OperationControl,
    ) -> wasmdroid_engine::Result<Inspection> {
        inspect(file, control).map_err(|_error| {
            // Keep cancellation typed and never echo untrusted manifest strings.
            control.check().err().unwrap_or_else(|| {
                EngineError::Invalid(
                    "local package format, metadata or module validation failed".into(),
                )
            })
        })
    }
}
fn inspect(file: &File, control: &OperationControl) -> Result<Inspection> {
    control.check()?;
    let bytes = OwnedBuffer::from_file(file)?;
    if wasmparser::Parser::is_core_wasm(bytes.as_slice()) {
        validate_wasm(bytes.as_slice(), true, control)?;
        return Ok(Inspection {
            kind: ArtifactKind::Wasm,
            commands: vec!["main".into()],
            entrypoint: Some("main".into()),
            dependencies: vec![],
        });
    }
    let container = inspect_webc(bytes, control)?;
    ensure!(
        container.manifest().atoms.len() <= 256
            && container.manifest().commands.len() <= 256
            && container.manifest().use_map.len() <= 32,
        "package metadata count limit exceeded"
    );
    let info = PackageInfo::from_manifest(
        PackageId::Hash(PackageHash::from_sha256_bytes([0; 32])),
        container.manifest(),
        container.version(),
    )?;
    ensure!(
        info.dependencies.len() <= 32 && info.commands.len() <= 256,
        "package metadata exceeds limits"
    );
    let mut dependencies = Vec::new();
    for dependency in info.dependencies {
        control.check()?;
        match dependency.pkg {
            PackageSource::Ident(ident) => {
                if let Some(named) = ident.as_named() {
                    ensure!(
                        named.tag.is_none() || named.version_opt().is_some(),
                        "named dependency tags require registry resolution and are unsupported locally"
                    );
                    ensure!(
                        named.registry.as_deref().is_none_or(|r| r == "wasmer.io"),
                        "unsupported dependency registry"
                    );
                    dependencies.push(Dependency::Named {
                        reference: wasmdroid_engine::api::ArtifactReference::parse(format!(
                            "wasmer.io/{}",
                            named.full_name()
                        ))?,
                        version_requirement: named.version_or_default().to_string(),
                    });
                } else if let Some(hash) = ident.as_hash() {
                    dependencies.push(Dependency::Sha256 {
                        digest: Digest::parse(hash.to_string())?,
                    });
                } else {
                    anyhow::bail!("unsupported dependency identity");
                }
            }
            _ => anyhow::bail!("local file and URL dependencies are not supported"),
        }
    }
    for command in container.manifest().commands.values() {
        ensure!(
            command.runner == "https://webc.org/runner/wasi",
            "unsupported command runner"
        );
        let atom = command.atom()?.context("command has no atom binding")?;
        if let Some(dependency) = atom.dependency {
            ensure!(
                container.manifest().use_map.contains_key(&dependency),
                "command refers to an undeclared dependency"
            );
        } else {
            ensure!(
                container.get_atom(&atom.name).is_some(),
                "command atom is missing"
            );
        }
    }
    let atoms = container.atoms();
    ensure!(
        atoms.len() == container.manifest().atoms.len(),
        "declared atom set differs from its contents"
    );
    for (name, atom) in atoms {
        control.check()?;
        ensure!(
            wasmer_wasix::runtime::package_loader::to_module_hash(
                container.manifest().atom_signature(&name)?
            ) == wasmer_types::ModuleHash::new(&atom),
            "atom digest mismatch"
        );
        validate_wasm(&atom, false, control)?;
    }
    Ok(Inspection {
        kind: ArtifactKind::Webc,
        commands: info
            .commands
            .into_iter()
            .map(|command| command.name)
            .collect(),
        entrypoint: info.entrypoint,
        dependencies,
    })
}

/// Bound parser work before serde or the WebC reader sees attacker-controlled
/// CBOR. Semantic validity remains the responsibility of those format readers.
fn cbor_item(
    bytes: &[u8],
    cursor: &mut usize,
    depth: usize,
    nodes: &mut usize,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    ensure!(
        depth <= 32 && *nodes < 65_536,
        "metadata nesting/item limit exceeded"
    );
    *nodes += 1;
    let first = *bytes.get(*cursor).context("truncated CBOR")?;
    *cursor += 1;
    let major = first >> 5;
    let extra = first & 31;
    let count = match extra {
        0..=23 => Some(extra as u64),
        24..=27 => {
            let length = 1usize << (extra - 24);
            let end = cursor.checked_add(length).context("CBOR length overflow")?;
            let raw = bytes.get(*cursor..end).context("truncated CBOR length")?;
            *cursor = end;
            Some(
                raw.iter()
                    .fold(0u64, |value, byte| (value << 8) | u64::from(*byte)),
            )
        }
        31 if matches!(major, 2..=5) => None,
        _ => anyhow::bail!("invalid CBOR argument"),
    };
    match (major, count) {
        (0 | 1 | 7, Some(_)) => {}
        (2 | 3, Some(length)) => {
            *cursor = cursor
                .checked_add(usize::try_from(length)?)
                .context("CBOR length overflow")?;
            ensure!(*cursor <= bytes.len(), "truncated CBOR string");
        }
        (4 | 5, Some(length)) => {
            let items = length
                .checked_mul(if major == 5 { 2 } else { 1 })
                .context("CBOR item overflow")?;
            ensure!(items <= 65_536, "metadata item limit exceeded");
            for _ in 0..items {
                cbor_item(bytes, cursor, depth + 1, nodes, control)?;
            }
        }
        (6, Some(_)) => cbor_item(bytes, cursor, depth + 1, nodes, control)?,
        (2..=5, None) => {
            let mut children = 0usize;
            while bytes.get(*cursor) != Some(&0xff) {
                if major <= 3 {
                    let next = *bytes.get(*cursor).context("truncated CBOR string")?;
                    ensure!(
                        next >> 5 == major && next & 31 != 31,
                        "invalid CBOR string chunk"
                    );
                }
                cbor_item(bytes, cursor, depth + 1, nodes, control)?;
                children += 1;
            }
            *cursor += 1;
            ensure!(major != 5 || children.is_multiple_of(2), "invalid CBOR map");
        }
        _ => anyhow::bail!("invalid CBOR item"),
    }
    Ok(())
}
fn bounded_cbor(bytes: &[u8], control: &OperationControl) -> Result<()> {
    ensure!(
        bytes.len() <= 4 * 1024 * 1024,
        "metadata byte limit exceeded"
    );
    cbor_item(bytes, &mut 0, 0, &mut 0, control)
}
fn u64_le(bytes: &[u8], start: usize) -> Result<usize> {
    let raw: [u8; 8] = bytes
        .get(start..start.checked_add(8).context("section overflow")?)
        .context("truncated section length")?
        .try_into()?;
    Ok(usize::try_from(u64::from_le_bytes(raw))?)
}

pub(crate) fn inspect_webc(bytes: OwnedBuffer, control: &OperationControl) -> Result<Container> {
    ensure!(
        bytes.as_slice().get(..8) == Some(b"\0webc003"),
        "only WebC v3 and core Wasm are supported"
    );
    // Sequential framing prevents an index from pointing into a different
    // section's payload and bypassing preflight metadata limits.
    let mut sections = BTreeMap::new();
    let mut offset = 8usize;
    while offset < bytes.len() {
        control.check()?;
        ensure!(sections.len() < 259, "section count limit exceeded");
        let tag = bytes[offset];
        let header = if tag == Tag::Index as u8 { 9 } else { 41 };
        let size = u64_le(
            &bytes,
            offset.checked_add(header - 8).context("section overflow")?,
        )?;
        let data = offset.checked_add(header).context("section overflow")?;
        let end = data.checked_add(size).context("section overflow")?;
        ensure!(end <= bytes.len(), "truncated WebC section");
        if matches!(tag, t if t == Tag::Index as u8 || t == Tag::Manifest as u8) {
            bounded_cbor(&bytes[data..end], control)?;
        }
        sections.insert(offset, (tag, end - offset, data, end));
        offset = end;
    }
    let (_, _, start, end) = *sections
        .get(&8)
        .filter(|s| s.0 == Tag::Index as u8)
        .context("missing initial index")?;
    let index: Index = ciborium::from_reader(&bytes[start..end])?;
    ensure!(index.volumes.len() <= 256, "volume count limit exceeded");
    let check_span = |span: Span, tag: Tag| -> Result<(usize, usize)> {
        let &(actual, size, start, end) = sections
            .get(&span.start)
            .context("index points outside a section")?;
        ensure!(
            actual == tag as u8 && size == span.len,
            "index section mismatch"
        );
        Ok((start, end))
    };
    check_span(index.manifest.span, Tag::Manifest)?;
    let (start, end) = check_span(index.atoms.span, Tag::Atoms)?;
    // Inspect the lazy atom directory before OwnedReader collects it into a map.
    let section = Section::parse(Tag::Atoms as u8, Some([0; 32]), bytes.slice(start..end))?;
    let mut count = 0;
    let mut names = std::collections::BTreeSet::new();
    for entry in section.as_atoms().context("missing atoms")?.iter() {
        control.check()?;
        let (name, _, _) = entry?;
        count += 1;
        ensure!(
            count <= 256 && name.len() <= 128 && names.insert(name.to_owned()),
            "atom count/name limit exceeded"
        );
    }
    for entry in index.volumes.values() {
        check_span(entry.span, Tag::Volume)?;
    }
    Ok(Container::new(webc::v3::read::OwnedReader::parse(bytes)?))
}

/// Core validation is bounded and cancellation is checked between sections and
/// function bodies. This does not claim that every valid module links in WASIX.
pub(crate) fn validate_wasm(
    bytes: &[u8],
    require_start: bool,
    control: &OperationControl,
) -> Result<()> {
    use wasmparser::{ExternalKind, Parser, Payload, TypeRef, ValidPayload, Validator};
    ensure!(Parser::is_core_wasm(bytes), "expected a core Wasm module");
    let mut validator = Validator::new();
    let mut types = Vec::new();
    let mut functions = Vec::new();
    let mut start = None;
    for payload in Parser::new(0).parse_all(bytes) {
        control.check()?;
        let payload = payload?;
        if let Some((id, range)) = payload.as_section() {
            // Code/data/custom bodies are borrowed; the remaining sections can
            // allocate validator tables, type arrays and import/export names.
            ensure!(
                matches!(id, 0 | 10 | 11) || range.len() <= 4 * 1024 * 1024,
                "module metadata byte limit exceeded"
            );
        }
        match &payload {
            Payload::TypeSection(reader) => {
                ensure!(
                    reader.count() <= 65_536 && reader.range().len() <= 1024 * 1024,
                    "type section limit exceeded"
                );
                for ty in reader.clone().into_iter_err_on_gc_types() {
                    let ty = ty?;
                    types.push(ty.params().is_empty() && ty.results().is_empty());
                }
            }
            Payload::ImportSection(reader) => {
                let mut count = 0;
                for import in reader.clone().into_imports() {
                    let import = import?;
                    count += 1;
                    ensure!(count <= 1024, "import count limit exceeded");
                    if let TypeRef::Func(ty) = import.ty {
                        functions.push(ty);
                    }
                    if let TypeRef::Memory(memory) = import.ty {
                        ensure!(!memory.memory64, "wasm64 is unsupported");
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                ensure!(reader.count() <= 100_000, "function count limit exceeded");
                for ty in reader.clone() {
                    functions.push(ty?);
                }
            }
            Payload::ExportSection(reader) => {
                ensure!(reader.count() <= 4096, "export count limit exceeded");
                for export in reader.clone() {
                    let export = export?;
                    if export.name == "_start" && export.kind == ExternalKind::Func {
                        start = Some(export.index);
                    }
                }
            }
            Payload::MemorySection(reader) => {
                ensure!(reader.count() <= 16, "memory count limit exceeded");
                for memory in reader.clone() {
                    ensure!(!memory?.memory64, "wasm64 is unsupported");
                }
            }
            Payload::CodeSectionEntry(body) => {
                ensure!(
                    body.range().len() <= 1024 * 1024,
                    "function body byte limit exceeded"
                );
                let mut locals = 0u64;
                for local in body.get_locals_reader()? {
                    locals += u64::from(local?.0);
                    ensure!(locals <= 100_000, "function local limit exceeded");
                }
            }
            _ => {}
        }
        if let ValidPayload::Func(function, body) = validator.payload(&payload)? {
            function
                .into_validator(Default::default())
                .validate(&body)?;
        }
    }
    if require_start {
        let ty = start
            .and_then(|i| functions.get(i as usize))
            .and_then(|i| types.get(*i as usize));
        ensure!(
            ty == Some(&true),
            "a WASI _start function with no parameters/results is required"
        );
    }
    control.check()?;
    Ok(())
}

pub(crate) fn raw_package(bytes: Vec<u8>, digest: &Digest) -> Result<BinaryPackage> {
    validate_wasm(&bytes, true, &OperationControl::default())?;
    let id = PackageId::Hash(PackageHash::from_str(digest.as_str())?);
    let hash = wasmer_types::ModuleHash::new(&bytes);
    let command = BinaryPackageCommand::new(
        "main".into(),
        webc::metadata::Command {
            runner: "https://webc.org/runner/wasi".into(),
            annotations: Default::default(),
        },
        bytes.into(),
        hash,
        None,
        id.clone(),
        id.clone(),
    );
    Ok(BinaryPackage {
        package_ids: vec![id.clone()],
        id,
        when_cached: None,
        entrypoint_cmd: Some("main".into()),
        hash: Default::default(),
        package_mounts: None,
        commands: vec![command],
        uses: vec![],
        file_system_memory_footprint: 0,
        additional_host_mapped_directories: vec![],
    })
}
