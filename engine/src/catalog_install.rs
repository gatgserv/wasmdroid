//! Catalog group preparation uses the same managed-stack/data authority as
//! advanced imports. No guest starts here, and caller graph JSON grants nothing.
use crate::{
    EngineError, Result,
    api::InstanceSpec,
    catalog::{Catalog, CatalogPackage},
    session::EngineSession,
    stacks::{StackManager, StackRecord},
};
pub const ARTIFACT_DIGEST_LABEL: &str = "dev.wasmdroid.catalog.artifact_digest";

/// A signed recipe describes requested capabilities, never operator consent.
/// Normal installation prepares the same application with outbound grants off;
/// a subsequent reviewed, versioned configuration may grant either separately.
/// The package is not mutated, so its signature/request metadata stay intact.
pub fn effective_single_spec(package: &CatalogPackage) -> Result<InstanceSpec> {
    Catalog::parse(&serde_json::to_string(&Catalog {
        schema_version: if package.media.is_some() { 3 } else { 2 },
        packages: vec![package.clone()],
    })?)?;
    if !package.android_verified {
        return Err(EngineError::Unsupported(
            "curated preparation requires Android evidence".into(),
        ));
    }
    let mut spec = package
        .single_recipe()
        .ok_or_else(|| {
            EngineError::Invalid("catalog entry is not a single application recipe".into())
        })?
        .clone();
    spec.internet_access = false;
    spec.lan_access = false;
    spec.validate()?;
    Ok(spec)
}

pub fn prepare_stack(
    session: &EngineSession,
    id: &str,
    package: &CatalogPackage,
    validate_runtime: &mut dyn FnMut(&InstanceSpec) -> Result<()>,
) -> Result<StackRecord> {
    // Revalidate typed callers too; a deserialized enum is not approval.
    Catalog::parse(&serde_json::to_string(&Catalog {
        schema_version: if package.media.is_some() { 3 } else { 2 },
        packages: vec![package.clone()],
    })?)?;
    if !package.android_verified {
        return Err(EngineError::Unsupported(
            "curated preparation requires Android evidence".into(),
        ));
    }
    let recipe = package
        .stack_recipe()
        .ok_or_else(|| EngineError::Invalid("catalog entry is not a group recipe".into()))?;
    for expected in package.artifacts() {
        let actual = session.state().artifact(&expected.reference)?;
        if actual.digest != expected.digest {
            return Err(EngineError::Conflict(
                "catalog package digest changed before preparation".into(),
            ));
        }
    }
    let manager = StackManager::new(session);
    let mut plan = manager.preview(id, &recipe.document)?;
    let expected: std::collections::BTreeMap<_, _> = package
        .artifacts()
        .into_iter()
        .map(|artifact| (artifact.reference.as_str().to_owned(), artifact.digest))
        .collect();
    for (name, service) in &mut plan.services {
        if expected.get(service.spec.artifact_ref.as_str()) != Some(&service.digest) {
            return Err(EngineError::Conflict(
                "catalog package digest changed during planning".into(),
            ));
        }
        service
            .spec
            .labels
            .insert(ARTIFACT_DIGEST_LABEL.into(), service.digest.as_str().into());
        validate_runtime(&service.spec)?;
        service
            .spec
            .labels
            .insert("dev.wasmdroid.ui.package".into(), package.id.clone());
        if name == &recipe.primary_service {
            service
                .spec
                .labels
                .insert("dev.wasmdroid.ui.name".into(), package.name.clone());
            service
                .spec
                .labels
                .insert("dev.wasmdroid.catalog.primary".into(), "true".into());
            if let Some(web) = &package.web {
                service.spec.labels.insert(
                    "dev.wasmdroid.web.port".into(),
                    web.instance_port.to_string(),
                );
                service
                    .spec
                    .labels
                    .insert("dev.wasmdroid.web.path".into(), web.path.clone());
            }
        }
    }
    // Rechecks under storage admission and publishes all membership atomically.
    // New labelled datasets survive a failed publication for explicit recovery.
    manager.create_seeded_catalog_plan(&plan, &recipe.assets)
}
