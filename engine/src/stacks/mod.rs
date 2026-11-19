//! Compose subset planning and durable membership. Plans/records can include
//! private arguments/environment; never put them in events or diagnostics.
pub mod catalog;
mod compose;
mod update;
mod yaml;
use crate::{
    EngineError, Result,
    api::{ArtifactKind, InstanceSpec, Mount, MountSource, PortBinding},
    state::StateDb,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub use update::{MemberRevision, ServiceChange, StackEditSource, StackUpdatePreview};
pub(crate) use update::{current_by_name, require_stopped};
use yaml::{invalid, unsupported};

pub const MAX_SERVICES: usize = 8;
pub const STACK_LABEL: &str = "dev.wasmdroid.stack.id";
pub const SERVICE_LABEL: &str = "dev.wasmdroid.stack.service";
pub const LOCAL_METADATA_LABEL: &str = "dev.wasmdroid.stack.local_metadata";
const VOLUME_LABEL: &str = "dev.wasmdroid.stack.volume";
const MAX_PLAN_BYTES: usize = 96 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackPlan {
    pub schema_version: u32,
    pub id: String,
    pub endpoint_id: String,
    pub services: BTreeMap<String, PlannedService>,
    pub volumes: BTreeMap<String, PlannedVolume>,
    pub startup_order: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedService {
    pub instance_id: String,
    pub digest: crate::store::Digest,
    /// Imported dependency/entrypoint metadata is part of the reviewed local
    /// capability. Older public-only plans remain wire-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_metadata: Option<crate::store::Digest>,
    pub spec: InstanceSpec,
    pub depends_on: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedVolume {
    pub name: String,
    pub external: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct StackRecord {
    pub id: String,
    pub version: u64,
    pub plan: StackPlan,
    pub instances: Vec<crate::state::InstanceRecord>,
}

/// Read-only package inventory for an explicitly reviewed document. This is not
/// a plan and grants no download, filesystem or runtime authority.
#[derive(Serialize)]
pub struct StackPackageRequirement {
    pub reference: crate::api::ArtifactReference,
    pub services: Vec<String>,
    pub installed: bool,
    pub downloadable: bool,
}

pub fn package_requirements(
    document: &str,
    state: &StateDb,
) -> Result<Vec<StackPackageRequirement>> {
    let parsed = compose::parse(document)?;
    let mut requirements = BTreeMap::<String, StackPackageRequirement>::new();
    for (service, raw) in parsed.services {
        let reference = crate::api::ArtifactReference::parse(raw.image)?;
        let key = reference.as_str().to_owned();
        if let Some(existing) = requirements.get_mut(&key) {
            existing.services.push(service);
            continue;
        }
        let installed = match installed_stack_artifact(state, &reference) {
            Ok(_) => true,
            Err(EngineError::NotFound(_)) => false,
            Err(error) => return Err(error),
        };
        let downloadable = reference.as_str().starts_with("wasmer.io/");
        requirements.insert(
            key,
            StackPackageRequirement {
                reference,
                services: vec![service],
                installed,
                downloadable,
            },
        );
    }
    Ok(requirements.into_values().collect())
}

pub fn plan(id: &str, document: &str, state: &StateDb) -> Result<StackPlan> {
    crate::api::validate_name(id)?;
    if id.len() > 64 {
        return Err(invalid("stack identity exceeds 64 bytes"));
    }
    let parsed = compose::parse(document)?;
    let mut services = BTreeMap::new();
    let mut volumes = BTreeMap::new();
    let mut fixed_ports = BTreeSet::new();
    for (logical, external) in parsed.volumes {
        let (name, external) = match external {
            Some(name) => (name, true),
            None => (scoped_name(id, "v", &logical), false),
        };
        crate::api::validate_name(&name)?;
        if external {
            state.volume(&name)?;
        }
        volumes.insert(logical, PlannedVolume { name, external });
    }
    for (service, raw) in parsed.services {
        let reference = crate::api::ArtifactReference::parse(raw.image)?;
        let artifact = installed_stack_artifact(state, &reference)?;
        let local_metadata = local_metadata(&artifact)?;
        let (command, args) = match raw.command {
            Some(mut argv) => {
                if argv.is_empty() {
                    return Err(unsupported("empty command override"));
                }
                (argv.remove(0), argv)
            }
            None => {
                let command = artifact
                    .labels
                    .get("dev.wasmdroid.entrypoint")
                    .cloned()
                    .or_else(|| {
                        (artifact.commands.len() == 1).then(|| artifact.commands[0].clone())
                    })
                    .ok_or_else(|| invalid("package entrypoint is ambiguous; select a command"))?;
                (command, Vec::new())
            }
        };
        if !artifact.commands.contains(&command) {
            return Err(invalid("command is absent from installed package"));
        }
        let mut mounts = Vec::new();
        for raw in raw.mounts {
            let volume = volumes
                .get(&raw.source)
                .ok_or_else(|| invalid("mount references an undeclared named volume"))?;
            mounts.push(Mount {
                source: MountSource::Volume(volume.name.clone()),
                target: raw.target,
                read_only: raw.read_only,
            });
        }
        for port in &raw.ports {
            if port.host_port != 0 && !fixed_ports.insert(port.host_port) {
                return Err(invalid("duplicate fixed host port across services"));
            }
        }
        let mut spec: InstanceSpec = serde_json::from_value(serde_json::json!({
            "artifact_ref":reference,"command":command,"args":args,"env":raw.environment,
            "mounts":mounts,"ports":raw.ports,"working_dir":raw.working_dir,
            "tty":raw.tty,"stdin_open":raw.stdin_open,
            "labels":{(STACK_LABEL):id,(SERVICE_LABEL):service,"dev.wasmdroid.ui.name":service}
        }))?;
        if let Some(pin) = &local_metadata {
            spec.labels
                .insert(LOCAL_METADATA_LABEL.into(), pin.as_str().into());
        }
        spec.validate()?;
        if serde_json::to_vec(&spec)?.len() > 128 * 1024 {
            return Err(invalid("service specification exceeds worker limit"));
        }
        services.insert(
            service.clone(),
            PlannedService {
                instance_id: scoped_name(id, "s", &service),
                digest: artifact.digest,
                local_metadata,
                spec,
                depends_on: raw.depends_on,
            },
        );
    }
    let startup_order = order(&services)?;
    let result = StackPlan {
        schema_version: 1,
        id: id.into(),
        endpoint_id: "local".into(),
        services,
        volumes,
        startup_order,
    };
    result.validate(state)?;
    Ok(result)
}

fn scoped_name(id: &str, kind: &str, logical: &str) -> String {
    format!(
        "s-{id}-{kind}-{}",
        &hex::encode(Sha256::digest(logical.as_bytes()))[..32]
    )
}
fn installed_stack_artifact(
    state: &StateDb,
    reference: &crate::api::ArtifactReference,
) -> Result<crate::api::Artifact> {
    let local = reference.as_str().starts_with("local.wasmdroid/sha256/");
    if local {
        crate::store::Digest::parse(format!(
            "sha256:{}",
            reference
                .as_str()
                .strip_prefix("local.wasmdroid/sha256/")
                .unwrap()
        ))?;
    }
    if !local && !reference.as_str().starts_with("wasmer.io/") {
        return Err(unsupported(
            "only installed public or imported digest packages",
        ));
    }
    let artifact = state.artifact(reference)?;
    if artifact.driver != "wasmer-local" || (!local && artifact.kind != ArtifactKind::Webc) {
        return Err(unsupported("stack artifact driver/kind"));
    }
    if local {
        // Existing import authority validates exact content identity and the
        // complete installed-only metadata/dependency lock. Never fetch here.
        crate::local_import::graph(state, &artifact)?;
    }
    Ok(artifact)
}
fn local_metadata(artifact: &crate::api::Artifact) -> Result<Option<crate::store::Digest>> {
    if artifact
        .reference
        .as_str()
        .starts_with("local.wasmdroid/sha256/")
    {
        Ok(Some(crate::store::Digest::of(&serde_json::to_vec(
            artifact,
        )?)))
    } else {
        Ok(None)
    }
}
/// Called again with the worker's actual cloned artifact before graph/runtime
/// construction. This closes the common-preview to worker-start race without
/// treating descriptive labels as capability grants.
pub fn validate_worker_artifact_pin(
    spec: &InstanceSpec,
    artifact: &crate::api::Artifact,
) -> Result<()> {
    if let Some(expected) = spec
        .labels
        .get(crate::catalog_install::ARTIFACT_DIGEST_LABEL)
        && (expected != artifact.digest.as_str() || spec.artifact_ref != artifact.reference)
    {
        return Err(EngineError::Conflict(
            "reviewed catalog package changed before worker loading".into(),
        ));
    }
    let actual = local_metadata(artifact)?;
    let expected = spec.labels.get(LOCAL_METADATA_LABEL);
    let managed = spec.labels.contains_key(STACK_LABEL) || spec.labels.contains_key(SERVICE_LABEL);
    match (actual, expected) {
        (Some(actual), Some(expected)) if actual.as_str() == expected => Ok(()),
        (Some(_), None) if !managed => Ok(()),
        (None, None) => Ok(()),
        _ => Err(EngineError::Conflict(
            "reviewed local package metadata changed; review the group again".into(),
        )),
    }
}
fn order(services: &BTreeMap<String, PlannedService>) -> Result<Vec<String>> {
    let mut pending: BTreeSet<String> = services.keys().cloned().collect();
    let mut ordered = Vec::new();
    while !pending.is_empty() {
        let next = pending
            .iter()
            .find(|name| {
                services[*name]
                    .depends_on
                    .iter()
                    .all(|dep| ordered.contains(dep))
            })
            .cloned()
            .ok_or_else(|| invalid("dependency cycle or missing service"))?;
        pending.remove(&next);
        ordered.push(next);
    }
    Ok(ordered)
}

impl StackPlan {
    /// Revalidate deserialized plans before any persistent/runtime side effects.
    pub fn validate(&self, state: &StateDb) -> Result<()> {
        crate::api::validate_name(&self.id)?;
        if self.id.len() > 64
            || self.schema_version != 1
            || self.endpoint_id != "local"
            || self.services.is_empty()
            || self.services.len() > MAX_SERVICES
        {
            return Err(invalid("unsupported stack plan identity/version/capacity"));
        }
        if order(&self.services)? != self.startup_order {
            return Err(invalid("invalid dependency order"));
        }
        if self.volumes.len() > 32 || serde_json::to_vec(self)?.len() > MAX_PLAN_BYTES {
            return Err(invalid("normalized plan exceeds volume or IPC budget"));
        }
        let mut instances = BTreeSet::new();
        let mut aliases = BTreeSet::new();
        let mut volume_names = BTreeSet::new();
        let mut fixed_ports = BTreeSet::new();
        for (logical, volume) in &self.volumes {
            compose::logical_name(logical)?;
            crate::api::validate_name(&volume.name)?;
            if !volume.external && volume.name != scoped_name(&self.id, "v", logical) {
                return Err(invalid("invalid scoped volume"));
            }
            volume_names.insert(&volume.name);
            if volume.external {
                state.volume(&volume.name)?;
            }
        }
        for (name, service) in &self.services {
            if !aliases.insert(compose::service_alias(name)?) {
                return Err(invalid("ambiguous service aliases"));
            }
            service.spec.validate()?;
            if service.spec.ports.len() > 16
                || service.spec.mounts.len() > 32
                || service.spec.env.len() > 128
            {
                return Err(invalid("service exceeds supported Compose field budgets"));
            }
            if service.instance_id != scoped_name(&self.id, "s", name)
                || !instances.insert(&service.instance_id)
                || service.spec.labels.get(STACK_LABEL) != Some(&self.id)
                || service.spec.labels.get(SERVICE_LABEL) != Some(name)
                || service.spec.endpoint_id != "local"
            {
                return Err(invalid("invalid stack service ownership"));
            }
            if service.spec.network != crate::api::NetworkMode::Loopback
                || service.spec.internet_access
                || service.spec.lan_access
                || service.spec.restart_policy != crate::api::RestartPolicy::No
                || service.spec.start_on_boot
                || service.spec.resources.cpu_shares.is_some()
                || service.spec.resources.memory_max_bytes.is_some()
                || service.spec.resources.timeout_seconds.is_some()
            {
                return Err(unsupported("features outside the supported Compose subset"));
            }
            if service.depends_on.len() > MAX_SERVICES
                || service.depends_on.iter().collect::<BTreeSet<_>>().len()
                    != service.depends_on.len()
            {
                return Err(invalid("invalid dependencies"));
            }
            let artifact = installed_stack_artifact(state, &service.spec.artifact_ref)?;
            validate_worker_artifact_pin(&service.spec, &artifact)?;
            if artifact.digest != service.digest
                || local_metadata(&artifact)? != service.local_metadata
                || !artifact.commands.contains(&service.spec.command)
            {
                return Err(EngineError::Conflict(
                    "stack artifact changed or command is unavailable".into(),
                ));
            }
            for Mount { source, .. } in &service.spec.mounts {
                if !matches!(source,MountSource::Volume(name) if volume_names.contains(name)) {
                    return Err(unsupported("ungranted stack volume or host path"));
                }
            }
            let mut guest_ports = BTreeSet::new();
            for PortBinding {
                host_port,
                protocol,
                instance_port,
                ..
            } in &service.spec.ports
            {
                if !guest_ports.insert(instance_port) {
                    return Err(invalid("duplicate guest port publication"));
                }
                if *protocol != crate::api::Protocol::Tcp {
                    return Err(unsupported("UDP forwarding"));
                }
                if *host_port != 0 && !fixed_ports.insert(*host_port) {
                    return Err(invalid("duplicate fixed host port"));
                }
            }
        }
        Ok(())
    }
}

pub struct StackManager<'a> {
    session: &'a crate::session::EngineSession,
}
impl<'a> StackManager<'a> {
    pub fn new(session: &'a crate::session::EngineSession) -> Self {
        Self { session }
    }
    pub fn preview(&self, id: &str, document: &str) -> Result<StackPlan> {
        plan(id, document, self.session.state())
    }
    pub fn create(&self, id: &str, document: &str) -> Result<StackRecord> {
        let plan = self.preview(id, document)?;
        self.create_plan(&plan)
    }
    pub fn create_plan(&self, plan: &StackPlan) -> Result<StackRecord> {
        plan.validate(self.session.state())?;
        let store = crate::store::ContentStore::open(self.session.root().join("store"))?;
        let _store_lease = store.installation_lease()?;
        let state = self.session.state();
        plan.validate(state)?;
        match state.stack(&plan.id) {
            Ok(_) => {
                return Err(EngineError::Conflict(
                    "stack identity already exists".into(),
                ));
            }
            Err(EngineError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
        for service in plan.services.values() {
            match state.instance(&service.instance_id) {
                Ok(_) => {
                    return Err(EngineError::Conflict(
                        "stack member identity already exists".into(),
                    ));
                }
                Err(EngineError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let _leases = self.prepare_volumes(plan)?;
        state.publish_stack(plan)
    }
    pub(crate) fn create_seeded_catalog_plan(
        &self,
        plan: &StackPlan,
        assets: &[crate::catalog_assets::CatalogSeedFile],
    ) -> Result<StackRecord> {
        if assets.is_empty() {
            return self.create_plan(plan);
        }
        crate::catalog_assets::validate(assets)?;
        plan.validate(self.session.state())?;
        let store = crate::store::ContentStore::open(self.session.root().join("store"))?;
        let _store_lease = store.installation_lease()?;
        let state = self.session.state();
        plan.validate(state)?;
        if state.stack_was_published(&plan.id)? {
            return Err(EngineError::Conflict(
                "catalog seed identity was already published; choose a new application".into(),
            ));
        }
        match state.stack(&plan.id) {
            Err(EngineError::NotFound(_)) => {}
            Ok(_) => {
                return Err(EngineError::Conflict(
                    "stack identity already exists".into(),
                ));
            }
            Err(error) => return Err(error),
        }
        for service in plan.services.values() {
            match state.instance(&service.instance_id) {
                Err(EngineError::NotFound(_)) => {}
                Ok(_) => {
                    return Err(EngineError::Conflict(
                        "stack member identity already exists".into(),
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        let pin = crate::catalog_assets::manifest_digest(assets)?;
        let seeded: BTreeSet<_> = assets.iter().map(|file| file.volume.as_str()).collect();
        if seeded.iter().any(|name| !plan.volumes.contains_key(*name)) {
            return Err(invalid("seed file requires a declared scoped volume"));
        }
        let manager = crate::volumes::VolumeManager::open(self.session)?;
        let mut missing = Vec::new();
        let mut leases = BTreeMap::new();
        // Validate every existing dataset/receipt before creating any new one.
        for (logical, volume) in &plan.volumes {
            if volume.external {
                return Err(unsupported("catalog seeds cannot use external datasets"));
            }
            match manager.inspect(&volume.name) {
                Ok(record) => {
                    if record.labels.get(STACK_LABEL) != Some(&plan.id)
                        || record.labels.get(VOLUME_LABEL) != Some(logical)
                        || (seeded.contains(logical.as_str())
                            && record
                                .labels
                                .get(crate::catalog_assets::SEED_LABEL)
                                .map(String::as_str)
                                != Some(pin.as_str()))
                    {
                        return Err(EngineError::Conflict(
                            "prepared catalog dataset identity changed".into(),
                        ));
                    }
                    leases.insert(logical.clone(), manager.exclusive(&volume.name)?);
                }
                Err(EngineError::NotFound(_)) => missing.push((logical, volume)),
                Err(error) => return Err(error),
            }
        }
        for (logical, volume) in missing {
            let mut labels = BTreeMap::from([
                (STACK_LABEL.into(), plan.id.clone()),
                (VOLUME_LABEL.into(), logical.clone()),
            ]);
            if seeded.contains(logical.as_str()) {
                labels.insert(
                    crate::catalog_assets::SEED_LABEL.into(),
                    pin.as_str().into(),
                );
            }
            manager.create_new(&volume.name, labels)?;
            leases.insert(logical.clone(), manager.exclusive(&volume.name)?);
        }
        let cache = self.session.root().join("transfers/cache");
        std::fs::create_dir_all(&cache)?;
        let staging = cap_std::fs::Dir::open_ambient_dir(cache, cap_std::ambient_authority())?;
        for logical in seeded {
            let files: Vec<_> = assets
                .iter()
                .filter(|file| file.volume == logical)
                .cloned()
                .collect();
            crate::catalog_assets::seed(&leases[logical], &files, &staging)?;
        }
        // Exclusive dataset capabilities stay live until actual SQL publication.
        state.publish_stack(plan)
    }
    fn prepare_volumes(&self, plan: &StackPlan) -> Result<Vec<crate::volumes::VolumeDirectory>> {
        let manager = crate::volumes::VolumeManager::open(self.session)?;
        let mut leases = Vec::new();
        let mut missing = Vec::new();
        let mut held = BTreeSet::new();
        // Check every existing dataset before creating any new dataset. Leases
        // exclude deletion/admin replacement until membership is committed.
        for (logical, volume) in &plan.volumes {
            if !held.insert(&volume.name) {
                continue;
            }
            match manager.inspect(&volume.name) {
                Ok(record) => {
                    if !volume.external
                        && (record.labels.get(STACK_LABEL) != Some(&plan.id)
                            || record.labels.get(VOLUME_LABEL) != Some(logical))
                    {
                        return Err(EngineError::Conflict(
                            "scoped volume identity belongs to other data".into(),
                        ));
                    }
                    leases.push(manager.lease(&volume.name, true)?);
                }
                Err(EngineError::NotFound(_)) if !volume.external => {
                    missing.push((logical, volume))
                }
                Err(error) => return Err(error),
            }
        }
        for (logical, volume) in missing {
            manager.create_new(
                &volume.name,
                BTreeMap::from([
                    (STACK_LABEL.into(), plan.id.clone()),
                    (VOLUME_LABEL.into(), logical.clone()),
                ]),
            )?;
            leases.push(manager.lease(&volume.name, true)?);
        }
        // IO/interruption can leave newly prepared, labelled volumes. Preserve
        // them for retry/recovery; never delete possibly written data to roll back.
        // No stack/member rows or member events escape a failed SQL transaction.
        Ok(leases)
    }
}
