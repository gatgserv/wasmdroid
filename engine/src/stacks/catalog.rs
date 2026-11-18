//! Pure catalog validation. No StateDb, package resolution, file access, download
//! or dataset authority is needed to reject an unsupported document.
use super::{compose, order, scoped_name, yaml::invalid, yaml::unsupported};
use crate::{
    Result,
    api::{ArtifactReference, InstanceSpec, Mount, MountSource, PortBinding},
    catalog::{CatalogArtifact, CatalogStackRecipe},
};
use std::collections::{BTreeMap, BTreeSet};

fn exact_public_reference(reference: &ArtifactReference) -> Result<()> {
    let path = reference
        .as_str()
        .strip_prefix("wasmer.io/")
        .ok_or_else(|| unsupported("catalog stacks require public Wasmer packages"))?;
    let (name, version) = path
        .rsplit_once('@')
        .ok_or_else(|| invalid("catalog stack images require an exact semantic version"))?;
    if name.split('/').count() != 2
        || name.contains(['@', ':'])
        || !semver::Version::parse(version).is_ok_and(|parsed| parsed.to_string() == version)
    {
        return Err(invalid(
            "catalog stack images require namespace/package@exact-semver",
        ));
    }
    Ok(())
}

fn parsed(recipe: &CatalogStackRecipe) -> Result<compose::Document> {
    let parsed = compose::parse(&recipe.document)?;
    if !parsed.services.contains_key(&recipe.primary_service) {
        return Err(invalid("catalog primary service is absent"));
    }
    Ok(parsed)
}

pub fn primary_artifact(recipe: &CatalogStackRecipe) -> Result<&CatalogArtifact> {
    let parsed = parsed(recipe)?;
    let image = &parsed.services[&recipe.primary_service].image;
    recipe
        .artifacts
        .iter()
        .find(|artifact| artifact.reference.as_str() == image)
        .ok_or_else(|| invalid("catalog primary artifact is absent"))
}

pub fn primary_ports(recipe: &CatalogStackRecipe) -> Result<Vec<PortBinding>> {
    Ok(parsed(recipe)?
        .services
        .remove(&recipe.primary_service)
        .unwrap()
        .ports)
}

/// Reuses the same strict YAML/JSON parser and neutral spec checks as planning.
/// Installed commands and immutable content are checked later against StateDb.
pub fn validate_recipe(recipe: &CatalogStackRecipe) -> Result<()> {
    crate::catalog_assets::validate(&recipe.assets)?;
    let parsed = parsed(recipe)?;
    if recipe
        .assets
        .iter()
        .any(|asset| !parsed.volumes.contains_key(&asset.volume))
    {
        return Err(invalid("seed file requires a declared scoped volume"));
    }
    if recipe.artifacts.is_empty() || recipe.artifacts.len() > super::MAX_SERVICES {
        return Err(invalid("catalog stack requires 1..8 declared artifacts"));
    }
    let mut artifacts = BTreeSet::new();
    for artifact in &recipe.artifacts {
        exact_public_reference(&artifact.reference)?;
        crate::store::Digest::parse(artifact.digest.as_str())?;
        crate::catalog_updates::public_url(&artifact.download_url)?;
        if !artifacts.insert(artifact.reference.as_str()) {
            return Err(invalid("duplicate catalog artifact reference"));
        }
    }
    if parsed.volumes.values().any(Option::is_some) {
        return Err(unsupported(
            "catalog stacks cannot grant external dataset authority",
        ));
    }
    let mut required = BTreeSet::new();
    let mut fixed_ports = BTreeSet::new();
    let mut services = BTreeMap::new();
    for (name, raw) in parsed.services {
        let reference = ArtifactReference::parse(raw.image)?;
        exact_public_reference(&reference)?;
        required.insert(reference.as_str().to_owned());
        let mut mounts = Vec::new();
        for mount in raw.mounts {
            if !parsed.volumes.contains_key(&mount.source) {
                return Err(invalid("mount references an undeclared named volume"));
            }
            mounts.push(Mount {
                source: MountSource::Volume(scoped_name("catalog", "v", &mount.source)),
                target: mount.target,
                read_only: mount.read_only,
            });
        }
        for port in &raw.ports {
            if port.host_port != 0 && !fixed_ports.insert(port.host_port) {
                return Err(invalid("duplicate fixed host port across services"));
            }
        }
        let (command, args) = match raw.command {
            Some(mut argv) if !argv.is_empty() => (argv.remove(0), argv),
            Some(_) => return Err(unsupported("empty command override")),
            // Placeholder is only for validation, never published or executed.
            None => ("catalog-entrypoint-check".into(), Vec::new()),
        };
        let spec: InstanceSpec = serde_json::from_value(serde_json::json!({
            "artifact_ref":reference,"command":command,"args":args,
            "env":raw.environment,"mounts":mounts,"ports":raw.ports,
            "working_dir":raw.working_dir,"tty":raw.tty,"stdin_open":raw.stdin_open
        }))?;
        spec.validate()?;
        services.insert(
            name,
            super::PlannedService {
                instance_id: String::new(),
                digest: crate::store::Digest::of(b"validation only"),
                local_metadata: None,
                spec,
                depends_on: raw.depends_on,
            },
        );
    }
    if required.iter().map(String::as_str).collect::<BTreeSet<_>>() != artifacts {
        return Err(invalid(
            "catalog artifacts must exactly match all service images",
        ));
    }
    // The shared dependency-order check rejects both missing names and cycles.
    order(&services)?;
    Ok(())
}
