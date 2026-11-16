use crate::{EngineError, Result, api::InstanceSpec, store::Digest};
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
    pub recipe: InstanceSpec,
    pub web: Option<WebLaunch>,
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
        if catalog.schema_version != 1 {
            return Err(EngineError::Unsupported(format!(
                "catalog schema {}",
                catalog.schema_version
            )));
        }
        let mut ids = BTreeSet::new();
        for package in &catalog.packages {
            crate::api::validate_name(&package.id)?;
            if !ids.insert(&package.id) {
                return Err(EngineError::Invalid("duplicate catalog ID".into()));
            }
            Digest::parse(package.digest.as_str())?;
            package.recipe.validate()?;
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
            if let Some(web) = &package.web
                && (!web.path.starts_with('/')
                    || web.path.starts_with("//")
                    || web.path.contains('\\')
                    || web.path.chars().any(char::is_control)
                    || !package.recipe.ports.iter().any(|p| {
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
