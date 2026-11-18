use super::{PackageRequirement, Registry, RegistryPackage, ResolvedArtifact, SearchPage};
use crate::operation::OperationControl;
use crate::{
    EngineError, Result,
    api::{Artifact, ArtifactKind, ArtifactReference},
    store::Digest,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::Arc;
use std::{collections::BTreeMap, io::Read};

pub const REGISTRY_ENDPOINT: &str = "https://registry.wasmer.io/graphql";
const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const RESOLVE_QUERY: &str = include_str!("../../graphql/resolve.graphql");
const SEARCH_QUERY: &str = include_str!("../../graphql/search.graphql");
const PACKAGE_NAMES_QUERY: &str = include_str!("../../graphql/search-package-names.graphql");
const PACKAGE_NAMES_CURSOR: &str = "package-names:";
const PUBLIC_VERSION_QUERY: &str = include_str!("../../graphql/public-version.graphql");

pub struct WasmerRegistry {
    http: super::http::Http,
}

impl WasmerRegistry {
    pub fn new() -> Result<Self> {
        Self::with_control(Arc::new(OperationControl::default()))
    }
    /// Share this token with the controlled Installer operation.
    pub fn with_control(control: Arc<OperationControl>) -> Result<Self> {
        Ok(Self {
            http: super::http::Http::new(control)?,
        })
    }

    fn query<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<T> {
        let bytes = self.http.post_json(
            REGISTRY_ENDPOINT,
            &json!({"query":query,"variables":variables}),
            MAX_METADATA_BYTES,
        )?;
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

    /// Fetch a public release directly when the text index omits an exact match.
    pub fn find_public_version(
        &self,
        reference: &ArtifactReference,
    ) -> Result<Option<RegistryPackage>> {
        let (name, version) = split_reference(reference)?;
        let version =
            version.ok_or_else(|| EngineError::Invalid("exact release required".into()))?;
        let data: Value =
            self.query(PUBLIC_VERSION_QUERY, json!({"name":name,"version":version}))?;
        let node = &data["getPackageVersion"];
        if node["package"]["private"] != false || node.get("yankedAt") != Some(&Value::Null) {
            return Ok(None);
        }
        let package = decode_registry_package(node)?;
        Ok((package.reference == *reference).then_some(package))
    }

    fn search_page(
        &self,
        query: &str,
        cursor: Option<&str>,
        limit: u32,
        package_names: bool,
    ) -> Result<SearchPage> {
        let (document, field) = if package_names {
            (PACKAGE_NAMES_QUERY, "searchAutocomplete")
        } else {
            (SEARCH_QUERY, "search")
        };
        let data: Value = self.query(
            document,
            json!({"query":query,"after":cursor,"first":limit}),
        )?;
        decode_search_page(&data[field], package_names)
    }
}

impl Registry for WasmerRegistry {
    fn search(&self, query: &str, cursor: Option<&str>, limit: u32) -> Result<SearchPage> {
        if query.trim().is_empty() || query.len() > 256 || !(1..=50).contains(&limit) {
            return Err(EngineError::Invalid(
                "search needs 1..256 characters and a limit of 1..50".into(),
            ));
        }
        if let Some(cursor) = cursor.and_then(|value| value.strip_prefix(PACKAGE_NAMES_CURSOR)) {
            return self.search_page(query, Some(cursor), limit, true);
        }
        let text = self.search_page(query, cursor, limit, false)?;
        if cursor.is_none()
            && text.packages.iter().all(is_legacy_webc)
            && let Ok(names) = self.search_page(query, None, limit, true)
            && (names
                .packages
                .iter()
                .any(|package| !is_legacy_webc(package))
                || (text.packages.is_empty()
                    && (!names.packages.is_empty() || names.next_cursor.is_some())))
        {
            return Ok(names);
        }
        Ok(text)
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
        self.http
            .download(&artifact.download_url, artifact.artifact.size_bytes)
    }
}

fn is_legacy_webc(package: &RegistryPackage) -> bool {
    matches!(package.webc_version, Some(1 | 2))
}

fn decode_search_page(response: &Value, package_names: bool) -> Result<SearchPage> {
    let edges = response["edges"]
        .as_array()
        .ok_or_else(|| EngineError::Registry("missing search edges".into()))?;
    let mut packages = Vec::new();
    for edge in edges {
        let node = &edge["node"];
        if node["__typename"] != "PackageVersion" || node["package"]["private"] != false {
            continue;
        }
        if package_names
            && (node.get("yankedAt") != Some(&Value::Null)
                || !node["distribution"]["webcManifest"]
                    .as_str()
                    .and_then(|manifest| serde_json::from_str::<Value>(manifest).ok())
                    .and_then(|manifest| {
                        manifest["commands"]
                            .as_object()
                            .map(|commands| !commands.is_empty())
                    })
                    .unwrap_or(false))
        {
            continue;
        }
        packages.push(decode_registry_package(node)?);
    }
    let info = &response["pageInfo"];
    let next_cursor = if info["hasNextPage"] == true {
        info["endCursor"].as_str().map(|cursor| {
            if package_names {
                format!("{PACKAGE_NAMES_CURSOR}{cursor}")
            } else {
                cursor.to_owned()
            }
        })
    } else {
        None
    };
    Ok(SearchPage {
        packages,
        next_cursor,
    })
}

fn decode_registry_package(node: &Value) -> Result<RegistryPackage> {
    let package = &node["package"];
    let name = package_name(package)?;
    Ok(RegistryPackage {
        reference: ArtifactReference::parse(format!(
            "wasmer.io/{name}@{}",
            text(node, "version")?
        ))?,
        publisher: text(package, "namespace")?.into(),
        description: node["description"].as_str().unwrap_or("").into(),
        webc_version: match node["distribution"]["webcVersion"].as_str() {
            Some("V1") => Some(1),
            Some("V2") => Some(2),
            Some("V3") => Some(3),
            _ => None,
        },
        released_at: node["createdAt"].as_str().map(str::to_owned),
    })
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
    let manifest_commands = manifest["commands"]
        .as_object()
        .ok_or_else(|| EngineError::Registry("package has no command manifest".into()))?;
    let commands = manifest_commands.keys().cloned().collect();
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
    if let Some(requirements) = declared_environment(manifest_commands) {
        labels.insert("dev.wasmdroid.command-required-env".into(), requirements);
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

fn declared_environment(commands: &serde_json::Map<String, Value>) -> Option<String> {
    let mut declared = BTreeMap::new();
    for (command, metadata) in commands {
        let Some(fields) = metadata["annotations"]["dev.wasmdroid.required-env"].as_array() else {
            continue;
        };
        if command.len() > 128 || fields.is_empty() || fields.len() > 32 || declared.len() >= 32 {
            continue;
        }
        let mut names = std::collections::BTreeSet::new();
        let fields = fields.iter().map(|field| {
            let name = field.get("name")?.as_str()?;
            let mut bytes = name.bytes();
            let first = bytes.next()?;
            if name.len() > 128
                || !(first.is_ascii_alphabetic() || first == b'_')
                || !bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
                || !names.insert(name)
            {
                return None;
            }
            let kind = field.get("kind").map_or(Some("text"), Value::as_str)?;
            matches!(kind, "text" | "secret" | "port")
                .then(|| json!({ "name": name, "kind": kind }))
        });
        if let Some(fields) = fields.collect::<Option<Vec<_>>>() {
            declared.insert(command, fields);
        }
    }
    if declared.is_empty() {
        return None;
    }
    let encoded = serde_json::to_string(&declared).ok()?;
    (encoded.len() <= 128 * 1024).then_some(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stores_only_explicit_valid_command_environment_requirements() {
        let commands = json!({
            "serve": {"annotations": {
                "wasi": {"env": ["DEFAULT=provided"]},
                "dev.wasmdroid.required-env": [
                    {"name": "SERVICE_PORT", "kind": "port"},
                    {"name": "TOKEN", "kind": "secret"},
                    {"name": "NAME"}
                ]
            }},
            "defaults_only": {"annotations": {"wasi": {"env": ["OTHER=one"]}}},
            "bad": {"annotations": {"dev.wasmdroid.required-env": [{"name": "A=B"}]}}
        });
        let raw = declared_environment(commands.as_object().unwrap()).unwrap();
        let settings: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            settings,
            json!({"serve": [
                {"name": "SERVICE_PORT", "kind": "port"},
                {"name": "TOKEN", "kind": "secret"},
                {"name": "NAME", "kind": "text"},
            ]})
        );
    }

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
    fn reads_the_release_creation_date_and_accepts_older_metadata() {
        let mut node = fixture();
        node["createdAt"] = "2025-08-20T01:02:03.123456Z".into();
        node["updatedAt"] = "2025-09-10T00:00:00Z".into();
        let release = decode_registry_package(&node).unwrap();
        assert_eq!(
            release.released_at.as_deref(),
            Some("2025-08-20T01:02:03.123456Z")
        );
        let legacy = decode_registry_package(&fixture()).unwrap();
        assert!(legacy.released_at.is_none());
        let payload = serde_json::to_value(legacy).unwrap();
        assert!(payload.get("released_at").is_none());
        assert!(
            serde_json::from_value::<RegistryPackage>(payload)
                .unwrap()
                .released_at
                .is_none()
        );
    }
    #[test]
    fn reports_the_actual_webc_distribution_version_for_search_results() {
        let mut node = fixture();
        assert_eq!(
            decode_registry_package(&node).unwrap().webc_version,
            Some(3)
        );
        node["distribution"]["webcVersion"] = json!("V2");
        assert_eq!(
            decode_registry_package(&node).unwrap().webc_version,
            Some(2)
        );
        node["distribution"] = Value::Null;
        assert_eq!(decode_registry_package(&node).unwrap().webc_version, None);
    }

    #[test]
    fn name_search_uses_manifest_commands_and_routes_its_own_cursor() {
        let mut runnable = fixture();
        runnable["__typename"] = json!("PackageVersion");
        let mut library = runnable.clone();
        library["distribution"]["webcManifest"] = json!("{\"commands\":{}}");
        let mut yanked = runnable.clone();
        yanked["yankedAt"] = json!("2026-01-01");
        let decoded = decode_search_page(
            &json!({
                "edges": [{"node":library},{"node":runnable},{"node":yanked}],
                "pageInfo": {"hasNextPage":true,"endCursor":"registry-cursor"}
            }),
            true,
        )
        .unwrap();
        assert_eq!(decoded.packages.len(), 1);
        assert_eq!(
            decoded.packages[0].reference.as_str(),
            "wasmer.io/example/site@1.2.3"
        );
        assert_eq!(
            decoded.next_cursor.as_deref(),
            Some("package-names:registry-cursor")
        );
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
