use crate::{
    EngineError, Result,
    api::{ArtifactReference, InstanceSpec},
    store::Digest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub packages: Vec<CatalogPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPackage {
    pub id: String,
    pub name: String,
    pub description: String,
    pub publisher: String,
    pub digest: Digest,
    /// HTTPS only. Installation verifies digest before publishing the blob.
    pub download_url: String,
    pub android_verified: bool,
    /// A verified recipe must carry a link to a committed Android test report.
    pub evidence: Option<String>,
    pub recipe: CatalogRecipe,
    pub web: Option<WebLaunch>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::catalog_media::optional_media"
    )]
    pub media: Option<crate::catalog_media::CatalogMedia>,
}

/// The single-instance JSON shape stays unchanged in both catalog versions.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CatalogRecipe {
    Single(InstanceSpec),
    Stack(CatalogStackRecipe),
}

// Recipes may contain private environment/arguments. Debug is never authority.
impl std::fmt::Debug for CatalogRecipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Single(_) => "Single(<redacted>)",
            Self::Stack(_) => "Stack(<redacted>)",
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogStackKind {
    Stack,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogStackRecipe {
    #[serde(rename = "type")]
    pub kind: CatalogStackKind,
    /// The existing strict Compose subset, never host filesystem authority.
    pub document: String,
    pub artifacts: Vec<CatalogArtifact>,
    pub primary_service: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<crate::catalog_assets::CatalogSeedFile>,
}

impl std::fmt::Debug for CatalogStackRecipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CatalogStackRecipe(<redacted>)")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogArtifact {
    pub reference: ArtifactReference,
    pub digest: Digest,
    pub download_url: String,
}

impl CatalogPackage {
    pub fn single_recipe(&self) -> Option<&InstanceSpec> {
        match &self.recipe {
            CatalogRecipe::Single(spec) => Some(spec),
            CatalogRecipe::Stack(_) => None,
        }
    }

    pub fn stack_recipe(&self) -> Option<&CatalogStackRecipe> {
        match &self.recipe {
            CatalogRecipe::Single(_) => None,
            CatalogRecipe::Stack(stack) => Some(stack),
        }
    }

    /// Declared installation roots only; transitive package requirements remain
    /// the controlled installer's responsibility, not a capability from JSON.
    pub fn artifacts(&self) -> Vec<CatalogArtifact> {
        match &self.recipe {
            CatalogRecipe::Single(spec) => vec![CatalogArtifact {
                reference: spec.artifact_ref.clone(),
                digest: self.digest.clone(),
                download_url: self.download_url.clone(),
            }],
            CatalogRecipe::Stack(stack) => stack.artifacts.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebLaunch {
    pub instance_port: u16,
    pub path: String,
}

impl Catalog {
    pub fn parse(json: &str) -> Result<Self> {
        let catalog: Self = serde_json::from_str(json)?;
        if !matches!(catalog.schema_version, 1..=3) {
            return Err(EngineError::Unsupported(format!(
                "catalog schema {}",
                catalog.schema_version
            )));
        }
        let mut ids = BTreeSet::new();
        for package in &catalog.packages {
            if serde_json::to_vec(package)?.len() > 128 * 1024 {
                return Err(EngineError::Invalid(
                    "catalog entry exceeds IPC budget".into(),
                ));
            }
            crate::api::validate_name(&package.id)?;
            if !ids.insert(&package.id) {
                return Err(EngineError::Invalid("duplicate catalog ID".into()));
            }
            Digest::parse(package.digest.as_str())?;
            if let Some(media) = &package.media {
                if catalog.schema_version != 3 {
                    return Err(EngineError::Unsupported(
                        "inline media requires catalog schema 3".into(),
                    ));
                }
                crate::catalog_media::validate(media)?;
            } else if catalog.schema_version == 3 && package.android_verified {
                return Err(EngineError::Invalid(
                    "verified schema 3 package requires icon and screenshots".into(),
                ));
            }
            match &package.recipe {
                CatalogRecipe::Single(spec) => spec.validate()?,
                CatalogRecipe::Stack(stack) => {
                    if catalog.schema_version < 2 {
                        return Err(EngineError::Unsupported(
                            "stack recipes require catalog schema 2 or 3".into(),
                        ));
                    }
                    crate::stacks::catalog::validate_recipe(stack)?;
                    let primary = crate::stacks::catalog::primary_artifact(stack)?;
                    if primary.digest != package.digest
                        || primary.download_url != package.download_url
                    {
                        return Err(EngineError::Invalid(
                            "catalog primary artifact differs from package pin".into(),
                        ));
                    }
                }
            }
            if package.name.trim().is_empty() || package.publisher.trim().is_empty() {
                return Err(EngineError::Invalid(
                    "catalog name and publisher are required".into(),
                ));
            }
            let url = url::Url::parse(&package.download_url)
                .map_err(|_| EngineError::Invalid("invalid catalog download URL".into()))?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
                || package.download_url.chars().any(char::is_whitespace)
            {
                return Err(EngineError::Invalid(
                    "catalog download URL must be HTTPS".into(),
                ));
            }
            if package.android_verified
                && package
                    .evidence
                    .as_ref()
                    .is_none_or(|s| s.trim().is_empty())
            {
                return Err(EngineError::Invalid(
                    "Android verification requires evidence".into(),
                ));
            }
            let ports = match &package.recipe {
                CatalogRecipe::Single(spec) => spec.ports.clone(),
                CatalogRecipe::Stack(stack) => crate::stacks::catalog::primary_ports(stack)?,
            };
            if let Some(web) = &package.web
                && (!web.path.starts_with('/')
                    || web.path.starts_with("//")
                    || web.path.contains('\\')
                    || web.path.chars().any(char::is_control)
                    || !ports.iter().any(|p| {
                        p.instance_port == web.instance_port
                            && p.protocol == crate::api::Protocol::Tcp
                    }))
            {
                return Err(EngineError::Invalid(
                    "web launch needs a local path and declared TCP port".into(),
                ));
            }
        }
        Ok(catalog)
    }
}
