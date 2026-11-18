//! Explicit stopped-definition updates. Private specs/documents never enter
//! diagnostics or events; the coordinator separately fences runtime ownership.
use super::*;
use crate::api::{InstanceState, MountSource};
use crate::state::InstanceRecord;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberRevision {
    pub instance_id: String,
    pub version: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceChange {
    Keep,
    Configure,
    Replace,
    Add,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackUpdatePreview {
    pub schema_version: u32,
    pub stack_version: u64,
    pub members: BTreeMap<String, MemberRevision>,
    pub plan: StackPlan,
    pub changes: BTreeMap<String, ServiceChange>,
    pub removed: Vec<String>,
    pub preserved_volume_count: u64,
}
#[derive(Serialize)]
pub struct StackEditSource {
    pub id: String,
    pub version: u64,
    pub members: BTreeMap<String, MemberRevision>,
    pub document: String,
}
fn members(stack: &StackRecord) -> BTreeMap<String, MemberRevision> {
    stack
        .plan
        .startup_order
        .iter()
        .zip(&stack.instances)
        .map(|(name, instance)| {
            (
                name.clone(),
                MemberRevision {
                    instance_id: instance.id.clone(),
                    version: instance.version,
                },
            )
        })
        .collect()
}
pub(crate) fn current_by_name(stack: &StackRecord) -> BTreeMap<&str, &InstanceRecord> {
    stack
        .plan
        .startup_order
        .iter()
        .zip(&stack.instances)
        .map(|(name, instance)| (name.as_str(), instance))
        .collect()
}
fn changes(stack: &StackRecord, plan: &StackPlan) -> Result<BTreeMap<String, ServiceChange>> {
    let previous = current_by_name(stack);
    plan.services
        .iter()
        .map(|(name, service)| {
            let change = match previous.get(name.as_str()) {
                None => ServiceChange::Add,
                Some(old) if old.spec.artifact_ref != service.spec.artifact_ref => {
                    ServiceChange::Replace
                }
                Some(old)
                    if serde_json::to_value(&old.spec)? != serde_json::to_value(&service.spec)? =>
                {
                    ServiceChange::Configure
                }
                Some(_) => ServiceChange::Keep,
            };
            Ok((name.clone(), change))
        })
        .collect()
}
/// Presentation stays with the existing logical service. These fields never
/// select membership, grant ports or prove catalog verification.
fn preserve_presentation(source: &crate::api::InstanceSpec, target: &mut crate::api::InstanceSpec) {
    for key in [
        "dev.wasmdroid.ui.name",
        "dev.wasmdroid.ui.package",
        "dev.wasmdroid.catalog.primary",
        "dev.wasmdroid.web.port",
        "dev.wasmdroid.web.path",
    ] {
        if let Some(value) = source.labels.get(key) {
            target.labels.insert(key.into(), value.clone());
        }
    }
}
impl StackUpdatePreview {
    pub fn validate_current(&self, state: &StateDb) -> Result<()> {
        self.check_current(state).map(|_| ())
    }
    pub(crate) fn check_current(&self, state: &StateDb) -> Result<StackRecord> {
        if self.schema_version != 1 || serde_json::to_vec(self)?.len() > 128 * 1024 {
            return Err(invalid("invalid stack update version or budget"));
        }
        self.plan.validate(state)?;
        let stack = state.stack(&self.plan.id)?;
        let removed: Vec<_> = stack
            .plan
            .services
            .keys()
            .filter(|name| !self.plan.services.contains_key(*name))
            .cloned()
            .collect();
        if stack.version != self.stack_version || members(&stack) != self.members {
            return Err(EngineError::Conflict(
                "stale stack or member revision; preview again".into(),
            ));
        }
        if self.changes != changes(&stack, &self.plan)?
            || self.removed != removed
            || self.preserved_volume_count != state.stack_preserved_volume_count(&self.plan.id)?
        {
            return Err(invalid("stack update review changed"));
        }
        Ok(stack)
    }
    pub(crate) fn is_noop(&self, stack: &StackRecord) -> Result<bool> {
        Ok(self.removed.is_empty()
            && self
                .changes
                .values()
                .all(|change| *change == ServiceChange::Keep)
            && serde_json::to_value(&self.plan)? == serde_json::to_value(&stack.plan)?)
    }
}
impl StackManager<'_> {
    pub fn preview_update(
        &self,
        id: &str,
        version: u64,
        document: &str,
    ) -> Result<StackUpdatePreview> {
        let stack = self.session.state().stack(id)?;
        if stack.version != version {
            return Err(EngineError::Conflict("stale stack revision".into()));
        }
        let mut plan = self.preview(id, document)?;
        for (name, instance) in current_by_name(&stack) {
            if let Some(service) = plan.services.get_mut(name) {
                preserve_presentation(&instance.spec, &mut service.spec);
                if instance
                    .spec
                    .labels
                    .contains_key(crate::catalog_install::ARTIFACT_DIGEST_LABEL)
                {
                    service.spec.labels.insert(
                        crate::catalog_install::ARTIFACT_DIGEST_LABEL.into(),
                        service.digest.as_str().into(),
                    );
                }
            }
        }
        let preview = StackUpdatePreview {
            schema_version: 1,
            stack_version: version,
            members: members(&stack),
            changes: changes(&stack, &plan)?,
            removed: stack
                .plan
                .services
                .keys()
                .filter(|name| !plan.services.contains_key(*name))
                .cloned()
                .collect(),
            preserved_volume_count: self.session.state().stack_preserved_volume_count(id)?,
            plan,
        };
        preview.check_current(self.session.state())?;
        Ok(preview)
    }
    pub fn update_plan(&self, preview: &StackUpdatePreview) -> Result<StackRecord> {
        let state = self.session.state();
        let previous = preview.check_current(state)?;
        require_stopped(&previous)?;
        state.require_group_network_idle(&previous.id)?;
        if preview.is_noop(&previous)? {
            return Ok(previous);
        }
        let store = crate::store::ContentStore::open(self.session.root().join("store"))?;
        let _store_lease = store.installation_lease()?;
        // Recheck after admission, before creating any new dataset.
        require_stopped(&preview.check_current(state)?)?;
        state.require_group_network_idle(&preview.plan.id)?;
        let manager = crate::volumes::VolumeManager::open(self.session)?;
        let _old_leases = state
            .stack_current_volume_names(&preview.plan.id)?
            .iter()
            .map(|name| manager.lease(name, true))
            .collect::<Result<Vec<_>>>()?;
        let _new_leases = self.prepare_volumes(&preview.plan)?;
        state.publish_stack_update(preview)
    }
    /// Export current actual specs, including managed retry identities/restored
    /// mounts. JSON is a Compose document, never a host shell or environment.
    pub fn edit_source(&self, id: &str) -> Result<StackEditSource> {
        let stack = self.session.state().stack(id)?;
        let mut volumes = stack.plan.volumes.clone();
        for instance in &stack.instances {
            for mount in &instance.spec.mounts {
                let MountSource::Volume(name) = &mount.source else {
                    return Err(unsupported("non-volume stack mount"));
                };
                if !volumes.values().any(|volume| &volume.name == name) {
                    let mut index = 1;
                    while volumes.contains_key(&format!("restored_{index}")) {
                        index += 1;
                    }
                    volumes.insert(
                        format!("restored_{index}"),
                        PlannedVolume {
                            name: name.clone(),
                            external: true,
                        },
                    );
                }
            }
        }
        if volumes.len() > 32 {
            return Err(unsupported(
                "current data references exceed Compose volume budget",
            ));
        }
        let definitions: BTreeMap<_, _> = volumes
            .iter()
            .map(|(logical, volume)| {
                (
                    logical,
                    if volume.external {
                        serde_json::json!({"external":true,"name":volume.name})
                    } else {
                        serde_json::json!({})
                    },
                )
            })
            .collect();
        let mut services = BTreeMap::new();
        let escape = |value: &str| value.replace('$', "$$");
        for (name, instance) in current_by_name(&stack) {
            let spec = &instance.spec;
            let command: Vec<_> = std::iter::once(&spec.command)
                .chain(&spec.args)
                .map(|value| escape(value))
                .collect();
            let environment: BTreeMap<_, _> = spec
                .env
                .iter()
                .map(|(name, value)| (name, escape(value)))
                .collect();
            let mounts=spec.mounts.iter().map(|mount| {
                let MountSource::Volume(actual)=&mount.source else {return Err(unsupported("non-volume stack mount"));};
                let logical=volumes.iter().find(|(_,volume)|&volume.name==actual).map(|(name,_)|name)
                    .ok_or_else(||invalid("current stack data mapping is absent"))?;
                Ok(serde_json::json!({"type":"volume","source":logical,"target":escape(&mount.target),"read_only":mount.read_only}))
            }).collect::<Result<Vec<_>>>()?;
            let ports:Vec<_>=spec.ports.iter().map(|port|serde_json::json!({"target":port.instance_port,"published":port.host_port,"protocol":"tcp","host_ip":"127.0.0.1"})).collect();
            let mut service = serde_json::json!({"image":spec.artifact_ref,"command":command,"environment":environment,
                "volumes":mounts,"ports":ports,"depends_on":stack.plan.services[name].depends_on,"tty":spec.tty,"stdin_open":spec.stdin_open});
            if let Some(directory) = &spec.working_dir {
                service["working_dir"] = serde_json::Value::String(escape(directory));
            }
            services.insert(name, service);
        }
        let document =
            serde_json::to_string(&serde_json::json!({"services":services,"volumes":definitions}))?;
        // Round-trip against the same strict parser: no unsupported current
        // field, private value or restored mount may disappear on editor open.
        let normalized = self.preview(id, &document)?;
        for (name, instance) in current_by_name(&stack) {
            let mut expected = instance.spec.clone();
            let mut exported = normalized.services[name].spec.clone();
            preserve_presentation(&instance.spec, &mut exported);
            // The next explicit review refreshes this internal metadata pin.
            // User-entered settings/restored data must still round-trip exactly.
            expected.labels.remove(super::LOCAL_METADATA_LABEL);
            exported.labels.remove(super::LOCAL_METADATA_LABEL);
            expected
                .labels
                .remove(crate::catalog_install::ARTIFACT_DIGEST_LABEL);
            exported
                .labels
                .remove(crate::catalog_install::ARTIFACT_DIGEST_LABEL);
            if serde_json::to_value(&exported)? != serde_json::to_value(&expected)? {
                return Err(unsupported(
                    "current service cannot be represented losslessly in Compose",
                ));
            }
        }
        Ok(StackEditSource {
            id: id.into(),
            version: stack.version,
            members: members(&stack),
            document,
        })
    }
}
pub(crate) fn require_stopped(stack: &StackRecord) -> Result<()> {
    if stack.instances.iter().any(|instance| {
        !matches!(
            instance.state,
            InstanceState::Created | InstanceState::Exited | InstanceState::Dead
        )
    }) {
        return Err(EngineError::Conflict(
            "stop all stack services before updating the definition".into(),
        ));
    }
    Ok(())
}
