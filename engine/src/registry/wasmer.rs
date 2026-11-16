use super::{PackageRequirement, Registry, RegistryPackage, ResolvedArtifact, SearchPage};
use crate::{
    EngineError, Result,
    api::{Artifact, ArtifactKind, ArtifactReference},
    store::Digest,
};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Read, time::Duration};

pub const REGISTRY_ENDPOINT: &str = "https://registry.wasmer.io/graphql";
const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const RESOLVE_QUERY: &str = include_str!("../../graphql/resolve.graphql");
const SEARCH_QUERY: &str = include_str!("../../graphql/search.graphql");

pub struct WasmerRegistry {
    client: Client,
}

impl WasmerRegistry {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .https_only(true)
            .user_agent(concat!("WasmDroid/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("redirect limit exceeded")
                } else if validate_download_url(attempt.url().as_str()).is_err() {
                    attempt.error("unsafe redirect target")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(network_error)?;
        Ok(Self { client })
    }

    fn query<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<T> {
        let response = self
            .client
            .post(REGISTRY_ENDPOINT)
            .json(&json!({"query":query,"variables":variables}))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(network_error)?;
        let mut bytes = Vec::new();
        response
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_METADATA_BYTES {
            return Err(EngineError::Registry(
                "metadata response exceeds limit".into(),
            ));
        }
        let envelope: Value = serde_json::from_slice(&bytes)?;
        if envelope
            .get("errors")
            .and_then(Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
        {
            // Server messages may contain temporary URLs. Do not leak them through UI/errors.
            return Err(EngineError::Registry("GraphQL request was rejected".into()));
        }
        let data = envelope
            .get("data")
            .ok_or_else(|| EngineError::Registry("missing GraphQL data".into()))?;
        Ok(serde_json::from_value(data.clone())?)
    }

    fn resolve_version(&self, name: &str, version: Option<&str>) -> Result<ResolvedArtifact> {
        let data: Value = self.query(RESOLVE_QUERY, json!({"name":name,"version":version}))?;
        let resolved = decode_resolved(name, &data["getPackageVersion"])?;
        if version.is_some_and(|requested| {
            split_reference(&resolved.artifact.reference)
                .ok()
                .and_then(|(_, v)| v)
                != Some(requested)
        }) {
            return Err(EngineError::Registry("requested version mismatch".into()));
        }
        Ok(resolved)
    }
}

impl Registry for WasmerRegistry {
    fn search(&self, query: &str, cursor: Option<&str>, limit: u32) -> Result<SearchPage> {
        if query.trim().is_empty() || query.len() > 256 || !(1..=50).contains(&limit) {
            return Err(EngineError::Invalid(
                "search needs 1..256 characters and a limit of 1..50".into(),
            ));
        }
        let data: Value = self.query(
            SEARCH_QUERY,
            json!({"query":query,"after":cursor,"first":limit}),
        )?;
        let edges = data["search"]["edges"]
            .as_array()
            .ok_or_else(|| EngineError::Registry("missing search edges".into()))?;
        let mut packages = Vec::new();
        for edge in edges {
            let node = &edge["node"];
            if node["__typename"] != "PackageVersion" || node["package"]["private"] != false {
                continue;
            }
            let name = package_name(&node["package"])?;
            packages.push(RegistryPackage {
                reference: ArtifactReference::parse(format!(
                    "wasmer.io/{name}@{}",
                    text(node, "version")?
                ))?,
                publisher: text(&node["package"], "namespace")?.into(),
                description: node["description"].as_str().unwrap_or("").into(),
            });
        }
        let info = &data["search"]["pageInfo"];
        let next_cursor = if info["hasNextPage"] == true {
            info["endCursor"].as_str().map(str::to_owned)
        } else {
            None
        };
        Ok(SearchPage {
            packages,
            next_cursor,
        })
    }

    fn resolve(&self, reference: &ArtifactReference) -> Result<ResolvedArtifact> {
        let (name, version) = split_reference(reference)?;
        if let Some(version) = version {
            semver::Version::parse(version).map_err(|_| {
                EngineError::Invalid("Wasmer version must be an exact semantic version".into())
            })?;
        }
        self.resolve_version(name, version)
    }

    fn resolve_requirement(&self, requirement: &PackageRequirement) -> Result<ResolvedArtifact> {
        let (name, version) = split_reference(&requirement.reference)?;
        if version.is_some() {
            return Err(EngineError::Invalid(
                "dependency name must not contain a version".into(),
            ));
        }
        let range = semver::VersionReq::parse(&requirement.version_requirement)
            .map_err(|_| EngineError::Unsupported("dependency version syntax".into()))?;
        let latest = self.resolve_version(name, None)?;
        let (_, latest_version) = split_reference(&latest.artifact.reference)?;
        if latest_version
            .and_then(|v| semver::Version::parse(v).ok())
            .is_some_and(|v| range.matches(&v))
        {
            return Ok(latest);
        }
        let data: Value = self.query("query Versions($name:String!){ getPackage(name:$name){ private versions { version yankedAt } } }", json!({"name":name}))?;
        if data["getPackage"]["private"] != false {
            return Err(EngineError::NotFound(name.into()));
        }
        let versions = data["getPackage"]["versions"]
            .as_array()
            .ok_or_else(|| EngineError::Registry("missing versions".into()))?;
        let selected = versions
            .iter()
            .filter(|v| v["yankedAt"].is_null())
            .filter_map(|v| {
                v["version"]
                    .as_str()
                    .and_then(|v| semver::Version::parse(v).ok())
            })
            .filter(|v| range.matches(v))
            .max()
            .ok_or_else(|| EngineError::NotFound(format!("no matching version for {name}")))?;
        self.resolve_version(name, Some(&selected.to_string()))
    }

    fn download(&self, artifact: &ResolvedArtifact) -> Result<Box<dyn Read + Send>> {
        validate_download_url(&artifact.download_url)?;
        let response = self
            .client
            .get(&artifact.download_url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(network_error)?;
        // The client enforces HTTPS for redirects too; inspect the final URL as well.
        validate_download_url(response.url().as_str())?;
        if response
            .content_length()
            .is_some_and(|n| n != artifact.artifact.size_bytes)
        {
            return Err(EngineError::Registry(
                "download size differs from registry metadata".into(),
            ));
        }
        Ok(Box::new(response))
    }
}

fn network_error(error: reqwest::Error) -> EngineError {
    EngineError::Network(error.without_url().to_string())
}

fn split_reference(reference: &ArtifactReference) -> Result<(&str, Option<&str>)> {
    let path = reference
        .as_str()
        .strip_prefix("wasmer.io/")
        .ok_or_else(|| EngineError::Unsupported("registry host; expected wasmer.io".into()))?;
    let (name, version) = path
        .rsplit_once('@')
        .map_or((path, None), |(n, v)| (n, Some(v)));
    if name.split('/').count() != 2 || name.contains(':') {
        return Err(EngineError::Invalid(
            "Wasmer package must be namespace/name".into(),
        ));
    }
    Ok((name, version))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| EngineError::Registry(format!("missing field {key}")))
}

fn package_name(package: &Value) -> Result<String> {
    Ok(format!(
        "{}/{}",
        text(package, "namespace")?,
        text(package, "packageName")?
    ))
}

pub fn validate_download_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value)
        .map_err(|_| EngineError::Invalid("invalid package download URL".into()))?;
    let private_host = match url.host() {
        Some(url::Host::Domain(host)) => {
            host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
        None => true,
    };
    if url.scheme() != "https"
        || private_host
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || value.chars().any(char::is_whitespace)
    {
        return Err(EngineError::Invalid(
            "download requires public HTTPS without embedded credentials".into(),
        ));
    }
    Ok(())
}

fn decode_resolved(requested_name: &str, package: &Value) -> Result<ResolvedArtifact> {
    if package.is_null() || package["package"]["private"] != false {
        return Err(EngineError::NotFound(requested_name.into()));
    }
    if !package["yankedAt"].is_null() {
        return Err(EngineError::Registry(
            "package version has been yanked".into(),
        ));
    }
    let version = text(package, "version")?;
    semver::Version::parse(version)
        .map_err(|_| EngineError::Registry("invalid package version".into()))?;
    if package_name(&package["package"])? != requested_name {
        return Err(EngineError::Registry("package identity mismatch".into()));
    }
    let dist = &package["distribution"];
    if dist["webcVersion"] != "V3" {
        return Err(EngineError::Unsupported(
            "registry must provide WebC v3".into(),
        ));
    }
    let raw_digest = text(dist, "webcSha256Hash")?;
    let digest = Digest::parse(if raw_digest.starts_with("sha256:") {
        raw_digest.into()
    } else {
        format!("sha256:{raw_digest}")
    })?;
    let download_url = text(dist, "webcDownloadUrl")?.to_owned();
    validate_download_url(&download_url)?;
    let size_bytes = dist["webcSize"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| EngineError::Registry("invalid WebC size".into()))?;
    let manifest: Value = serde_json::from_str(text(dist, "webcManifest")?)?;
    let commands = manifest["commands"]
        .as_object()
        .ok_or_else(|| EngineError::Registry("package has no command manifest".into()))?
        .keys()
        .cloned()
        .collect();
    let mut dependencies = Vec::new();
    if let Some(uses) = manifest["use"].as_object() {
        for value in uses.values() {
            let identifier = value
                .as_str()
                .ok_or_else(|| EngineError::Unsupported("non-registry WebC dependency".into()))?;
            let (name, version_requirement) =
                identifier.rsplit_once('@').unwrap_or((identifier, "*"));
            let reference = ArtifactReference::parse(format!("wasmer.io/{name}"))?;
            split_reference(&reference)?;
            semver::VersionReq::parse(version_requirement)
                .map_err(|_| EngineError::Unsupported("WebC dependency constraint".into()))?;
            dependencies.push(PackageRequirement {
                reference,
                version_requirement: version_requirement.into(),
            });
        }
    }
    let mut labels = BTreeMap::new();
    if let Some(license) = package["license"].as_str() {
        labels.insert("org.opencontainers.image.licenses".into(), license.into());
    }
    if let Some(entrypoint) = manifest["entrypoint"].as_str() {
        labels.insert("dev.wasmdroid.entrypoint".into(), entrypoint.into());
    }
    Ok(ResolvedArtifact {
        artifact: Artifact {
            reference: ArtifactReference::parse(format!("wasmer.io/{requested_name}@{version}"))?,
            digest,
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands,
            size_bytes,
            labels,
        },
        download_url,
        dependencies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"version":"1.2.3","license":"MIT","yankedAt":null,
            "package":{"namespace":"example","packageName":"site","private":false},
            "distribution":{"webcVersion":"V3","webcSize":4,"webcSha256Hash":Digest::of(b"webc").as_str(),
                "webcDownloadUrl":"https://cdn.example.invalid/blob?temporary=fixture",
                "webcManifest":json!({"entrypoint":"serve","commands":{"serve":{}},"use":{"shell":"example/shell@^1.0"}}).to_string()}})
    }
    #[test]
    fn decodes_pinned_identity_digest_commands_and_dependencies_without_leaking_url() {
        let resolved = decode_resolved("example/site", &fixture()).unwrap();
        assert_eq!(
            resolved.artifact.reference.as_str(),
            "wasmer.io/example/site@1.2.3"
        );
        assert_eq!(resolved.artifact.commands, vec!["serve"]);
        assert_eq!(resolved.dependencies[0].version_requirement, "^1.0");
        assert!(!format!("{resolved:?}").contains("temporary=fixture"));
    }
    #[test]
    fn rejects_private_yanked_identity_mismatch_and_legacy_container() {
        let mut private = fixture();
        private["package"]["private"] = true.into();
        assert!(decode_resolved("example/site", &private).is_err());
        let mut yanked = fixture();
        yanked["yankedAt"] = "2026-01-01".into();
        assert!(decode_resolved("example/site", &yanked).is_err());
        assert!(decode_resolved("other/site", &fixture()).is_err());
        let mut legacy = fixture();
        legacy["distribution"]["webcVersion"] = "V2".into();
        assert!(decode_resolved("example/site", &legacy).is_err());
    }
    #[test]
    fn rejects_unsafe_download_urls_and_allows_public_https_capabilities() {
        for url in [
            "http://cdn.example.org/file",
            "https://localhost/file",
            "https://127.0.0.1/file",
            "https://10.0.0.1/file",
            "https://[::1]/file",
            "https://u:p@cdn.example.org/file",
            "file:///tmp/pkg",
        ] {
            assert!(validate_download_url(url).is_err(), "{url}");
        }
        validate_download_url("https://cdn.example.org/file?temporary=fixture").unwrap();
    }
}
