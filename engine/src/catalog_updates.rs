//! Opt-in signed curated metadata. Hosting and trust roots are explicit owner inputs.
//! This module never installs packages, starts guests, or creates trust on first use.
use crate::{
    EngineError, Result,
    catalog::{Catalog, CatalogPackage},
    operation::OperationControl,
    session::EngineSession,
    store::Digest,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::signature::{ED25519, UnparsedPublicKey};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    sync::Arc,
};

pub const MAX_INDEX_BYTES: usize = 128 * 1024;
pub const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_PACKAGES: usize = 512;
const SIGNATURE_DOMAIN: &[u8] = b"WasmDroid curated catalog index v1\0";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogUpdateConfig {
    pub channel: String,
    /// Public, credential-free HTTPS. There is deliberately no production default.
    pub index_url: String,
    pub trusted_keys: Vec<CatalogKey>,
    /// A release may ship a floor; persisted accepted sequence additionally fences rollback.
    pub minimum_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogKey {
    pub id: String,
    /// Standard base64 of a raw 32-byte Ed25519 public key, never a private key.
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCatalogIndex {
    pub schema_version: u32,
    pub key_id: String,
    /// Exact signed UTF-8 JSON bytes, base64 encoded; no JSON canonicalization assumption.
    pub payload: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogManifest {
    pub schema_version: u32,
    pub channel: String,
    pub sequence: u64,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub catalog_url: String,
    pub catalog_bytes: u64,
    pub catalog_digest: Digest,
    /// Publisher attestations: actual evidence/license review remains a human gate.
    pub package_reviews: Vec<PackageReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReview {
    pub package_id: String,
    pub artifact_digest: Digest,
    pub license_review: String,
    pub android_evidence: String,
    /// Media provenance/license attestation for the exact signed catalog bytes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::catalog_media::optional_review"
    )]
    pub media_review: Option<String>,
    /// Schema-2 stack roots each need their own immutable review attestation.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_artifact_reviews"
    )]
    pub artifact_reviews: BTreeMap<String, ArtifactReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReview {
    pub digest: Digest,
    pub license_review: String,
    pub android_evidence: String,
}

fn unique_artifact_reviews<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, ArtifactReview>, D::Error> {
    struct Reviews;
    impl<'de> serde::de::Visitor<'de> for Reviews {
        type Value = BTreeMap<String, ArtifactReview>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("at most eight unique artifact reviews")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((reference, review)) = map.next_entry::<String, ArtifactReview>()? {
                if result.len() >= crate::stacks::MAX_SERVICES
                    || result.insert(reference, review).is_some()
                {
                    return Err(serde::de::Error::custom(
                        "duplicate artifact review or review capacity exceeded",
                    ));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Reviews)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogFreshness {
    Current,
    Expired,
    ClockBeforeAcceptance,
    BelowMinimumSequence,
}

#[derive(Debug, Serialize)]
pub struct CatalogSnapshot {
    pub manifest: CatalogManifest,
    pub catalog: Catalog,
    pub accepted_at_unix_seconds: u64,
    pub freshness: CatalogFreshness,
}

/// The handle retains the engine's ownership lease for metadata I/O.
pub struct CatalogUpdates {
    db: Connection,
    lock_path: std::path::PathBuf,
    config: CatalogUpdateConfig,
    _ownership: crate::session::EngineOwnership,
}

impl CatalogUpdateConfig {
    pub fn validate(&self) -> Result<()> {
        crate::api::validate_name(&self.channel)?;
        public_url(&self.index_url)?;
        if self.trusted_keys.is_empty() {
            return Err(EngineError::Unsupported(
                "catalog updates require owner-approved signing keys".into(),
            ));
        }
        if self.trusted_keys.len() > 16 || self.minimum_sequence > i64::MAX as u64 {
            return Err(EngineError::Invalid(
                "catalog trust configuration exceeds limits".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        for key in &self.trusted_keys {
            crate::api::validate_name(&key.id)?;
            if !ids.insert(&key.id) || decode(&key.public_key, 32)?.len() != 32 {
                return Err(EngineError::Invalid(
                    "invalid or duplicate catalog signing key".into(),
                ));
            }
        }
        Ok(())
    }
}

impl CatalogUpdates {
    pub fn open(session: &EngineSession, config: CatalogUpdateConfig) -> Result<Self> {
        config.validate()?;
        let path = session.root().join("catalog-updates.sqlite");
        if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.is_symlink()) {
            return Err(EngineError::Invalid(
                "catalog database must be a private regular file".into(),
            ));
        }
        let lock_path = session.root().join("catalog-updates.update.lock");
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::ZERO)?;
        db.execute_batch(
            "PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS catalog_snapshots (
                 channel TEXT PRIMARY KEY NOT NULL,
                 index_bytes BLOB NOT NULL,
                 catalog_bytes BLOB NOT NULL,
                 sequence INTEGER NOT NULL,
                 payload_digest TEXT NOT NULL,
                 accepted_at INTEGER NOT NULL
             );",
        )?;
        Ok(Self {
            db,
            lock_path,
            config,
            _ownership: session.ownership(),
        })
    }

    /// Offline reads reverify signatures/digests under the current configured keys.
    /// Expiry is visible and never silently transformed into a fresh update.
    pub fn snapshot(&self, now_unix_seconds: u64) -> Result<Option<CatalogSnapshot>> {
        self.read_snapshot(now_unix_seconds)
            .map(|value| value.map(|(snapshot, _)| snapshot))
    }

    fn read_snapshot(&self, now: u64) -> Result<Option<(CatalogSnapshot, Digest)>> {
        let row = self
            .db
            .query_row(
                "SELECT index_bytes,catalog_bytes,sequence,payload_digest,accepted_at
             FROM catalog_snapshots WHERE channel=?1",
                [&self.config.channel],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(index, catalog, sequence, stored_digest, accepted)| {
            let sequence = u64::try_from(sequence)
                .map_err(|_| EngineError::Invalid("negative catalog sequence".into()))?;
            let accepted = u64::try_from(accepted)
                .map_err(|_| EngineError::Invalid("negative catalog acceptance clock".into()))?;
            let (manifest, payload_digest) = verify_index(&self.config, &index, false)?;
            if sequence != manifest.sequence || stored_digest != payload_digest.as_str() {
                return Err(EngineError::Invalid(
                    "catalog snapshot metadata is inconsistent".into(),
                ));
            }
            let catalog = verify_catalog(&manifest, &catalog)?;
            let freshness = if manifest.sequence < self.config.minimum_sequence {
                CatalogFreshness::BelowMinimumSequence
            } else if now < accepted || now < manifest.issued_at_unix_seconds {
                CatalogFreshness::ClockBeforeAcceptance
            } else if now >= manifest.expires_at_unix_seconds {
                CatalogFreshness::Expired
            } else {
                CatalogFreshness::Current
            };
            Ok((
                CatalogSnapshot {
                    manifest,
                    catalog,
                    accepted_at_unix_seconds: accepted,
                    freshness,
                },
                payload_digest,
            ))
        })
        .transpose()
    }

    /// Production transport shares installation TLS, bounded resolver admission,
    /// timeouts, redirect validation, URL redaction and active I/O cancellation.
    pub fn refresh(
        &mut self,
        now: u64,
        control: &Arc<OperationControl>,
    ) -> Result<CatalogSnapshot> {
        self.refresh_with(now, control, |url, limit, token| {
            crate::registry::http::Http::new(token.clone())?.get_bounded(url, limit as u64)
        })
    }

    /// A transport boundary for hermetic regressions/platform adapters. Production
    /// callers use refresh; all returned bytes are independently bounded/verified.
    pub fn refresh_with(
        &mut self,
        now: u64,
        control: &Arc<OperationControl>,
        mut fetch: impl FnMut(&str, usize, &Arc<OperationControl>) -> Result<Vec<u8>>,
    ) -> Result<CatalogSnapshot> {
        let _operation = control.claim()?;
        if now > i64::MAX as u64 {
            return Err(EngineError::Invalid(
                "catalog clock exceeds supported range".into(),
            ));
        }
        // Fails promptly rather than allowing concurrent stale writers. Never hold
        // an engine-wide storage mutex during network requests.
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.lock_path)?;
        crate::locking::try_lock(&lock, false, "another catalog update is active")?;
        control.check()?;
        let existing = self.read_snapshot(now)?;
        if existing
            .as_ref()
            .is_some_and(|(snapshot, _)| now < snapshot.accepted_at_unix_seconds)
        {
            return Err(EngineError::Conflict(
                "catalog clock precedes last acceptance".into(),
            ));
        }
        let index = fetch(&self.config.index_url, MAX_INDEX_BYTES, control)?;
        control.check()?;
        let (manifest, payload_digest) = verify_index(&self.config, &index, true)?;
        if now < manifest.issued_at_unix_seconds || now >= manifest.expires_at_unix_seconds {
            return Err(EngineError::Invalid(
                "catalog index is not currently valid".into(),
            ));
        }
        if let Some((snapshot, previous_digest)) = &existing {
            if manifest.sequence < snapshot.manifest.sequence {
                return Err(EngineError::Conflict("catalog rollback rejected".into()));
            }
            if manifest.sequence == snapshot.manifest.sequence && payload_digest != *previous_digest
            {
                return Err(EngineError::Conflict(
                    "catalog sequence reused with different metadata".into(),
                ));
            }
        }
        let bytes = fetch(
            &manifest.catalog_url,
            manifest.catalog_bytes as usize,
            control,
        )?;
        control.check()?;
        let catalog = verify_catalog(&manifest, &bytes)?;
        let transaction = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO catalog_snapshots(channel,index_bytes,catalog_bytes,sequence,payload_digest,accepted_at)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(channel) DO UPDATE SET index_bytes=excluded.index_bytes,
             catalog_bytes=excluded.catalog_bytes,sequence=excluded.sequence,
             payload_digest=excluded.payload_digest,accepted_at=excluded.accepted_at",
            params![self.config.channel,index,bytes,manifest.sequence as i64,payload_digest.as_str(),now as i64],
        )?;
        control.begin_publication()?;
        transaction.commit()?;
        control.committed();
        Ok(CatalogSnapshot {
            manifest,
            catalog,
            accepted_at_unix_seconds: now,
            freshness: CatalogFreshness::Current,
        })
    }

    /// Pin selection to the exact reviewed snapshot. A concurrent replacement
    /// cannot substitute a recipe under the same package ID. Expired metadata
    /// remains available for offline display but cannot authorize a new install.
    pub fn package_for_install(
        &self,
        sequence: u64,
        catalog_digest: &Digest,
        id: &str,
        now: u64,
    ) -> Result<CatalogPackage> {
        let snapshot = self
            .snapshot(now)?
            .ok_or_else(|| EngineError::NotFound("accepted catalog".into()))?;
        if snapshot.freshness != CatalogFreshness::Current
            || snapshot.manifest.sequence != sequence
            || snapshot.manifest.catalog_digest != *catalog_digest
        {
            return Err(EngineError::Conflict(
                "catalog selection is stale; review the current catalog".into(),
            ));
        }
        snapshot
            .catalog
            .packages
            .into_iter()
            .find(|package| package.id == id)
            .ok_or_else(|| EngineError::NotFound("catalog package".into()))
    }
}

pub(crate) fn public_url(value: &str) -> Result<()> {
    crate::registry::wasmer::validate_download_url(value)?;
    // Metadata endpoints are stable public documents, never temporary signed
    // capabilities or authentication URLs. Do not retain query secrets on disk.
    if url::Url::parse(value)
        .map_err(|_| EngineError::Invalid("invalid catalog URL".into()))?
        .query()
        .is_some()
    {
        return Err(EngineError::Invalid(
            "catalog metadata URLs must not contain a query".into(),
        ));
    }
    Ok(())
}

fn decode(value: &str, maximum: usize) -> Result<Vec<u8>> {
    if value.len()
        > maximum
            .saturating_add(2)
            .saturating_div(3)
            .saturating_mul(4)
    {
        return Err(EngineError::Invalid(
            "catalog encoded field exceeds limit".into(),
        ));
    }
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| EngineError::Invalid("invalid catalog base64".into()))?;
    if bytes.len() > maximum {
        return Err(EngineError::Invalid(
            "catalog decoded field exceeds limit".into(),
        ));
    }
    Ok(bytes)
}

fn verify_index(
    config: &CatalogUpdateConfig,
    bytes: &[u8],
    enforce_floor: bool,
) -> Result<(CatalogManifest, Digest)> {
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(EngineError::Invalid("catalog index exceeds limit".into()));
    }
    let index: SignedCatalogIndex = serde_json::from_slice(bytes)?;
    if index.schema_version != 1 {
        return Err(EngineError::Unsupported("catalog signature schema".into()));
    }
    let key = config
        .trusted_keys
        .iter()
        .find(|key| key.id == index.key_id)
        .ok_or_else(|| EngineError::Invalid("catalog signature key is not trusted".into()))?;
    let payload = decode(&index.payload, MAX_INDEX_BYTES)?;
    let signature = decode(&index.signature, 64)?;
    let mut message = Vec::with_capacity(SIGNATURE_DOMAIN.len() + payload.len());
    message.extend_from_slice(SIGNATURE_DOMAIN);
    message.extend_from_slice(&payload);
    UnparsedPublicKey::new(&ED25519, decode(&key.public_key, 32)?)
        .verify(&message, &signature)
        .map_err(|_| EngineError::Invalid("catalog signature verification failed".into()))?;
    let manifest: CatalogManifest = serde_json::from_slice(&payload)?;
    if manifest.schema_version != 1 {
        return Err(EngineError::Unsupported("catalog manifest schema".into()));
    }
    if manifest.channel != config.channel
        || (enforce_floor && manifest.sequence < config.minimum_sequence)
        || manifest.sequence > i64::MAX as u64
        || manifest.expires_at_unix_seconds <= manifest.issued_at_unix_seconds
        || manifest.catalog_bytes == 0
        || manifest.catalog_bytes > MAX_CATALOG_BYTES as u64
        || manifest.package_reviews.len() > MAX_PACKAGES
    {
        return Err(EngineError::Invalid(
            "catalog manifest violates configured channel or bounds".into(),
        ));
    }
    public_url(&manifest.catalog_url)?;
    Ok((manifest, Digest::of(&payload)))
}

fn verify_catalog(manifest: &CatalogManifest, bytes: &[u8]) -> Result<Catalog> {
    if bytes.len() as u64 != manifest.catalog_bytes || bytes.len() > MAX_CATALOG_BYTES {
        return Err(EngineError::Invalid(
            "catalog length differs from signed manifest".into(),
        ));
    }
    let actual = Digest::of(bytes);
    if actual != manifest.catalog_digest {
        return Err(EngineError::DigestMismatch {
            expected: manifest.catalog_digest.as_str().into(),
            actual: actual.as_str().into(),
        });
    }
    let catalog = Catalog::parse(
        std::str::from_utf8(bytes)
            .map_err(|_| EngineError::Invalid("catalog must be UTF-8".into()))?,
    )?;
    if catalog.packages.len() > MAX_PACKAGES
        || catalog.packages.len() != manifest.package_reviews.len()
    {
        return Err(EngineError::Invalid(
            "catalog requires exactly one review per package".into(),
        ));
    }
    let mut reviewed = BTreeSet::new();
    for review in &manifest.package_reviews {
        let package = catalog
            .packages
            .iter()
            .find(|package| package.id == review.package_id)
            .ok_or_else(|| {
                EngineError::Invalid("catalog review refers to an absent package".into())
            })?;
        if !reviewed.insert(&review.package_id)
            || !package.android_verified
            || package.digest != review.artifact_digest
            || package.evidence.as_deref() != Some(review.android_evidence.as_str())
            || review.android_evidence.trim().is_empty()
            || review.license_review.trim().is_empty()
        {
            return Err(EngineError::Invalid(
                "catalog package lacks matching Android and license review attestations".into(),
            ));
        }
        public_url(&package.download_url)?;
        match (&package.media, &review.media_review) {
            (Some(_), Some(reference)) if crate::catalog_media::review_reference(reference) => {}
            (None, None) => {}
            _ => {
                return Err(EngineError::Invalid(
                    "catalog media requires a matching provenance review attestation".into(),
                ));
            }
        }
        let artifacts = package.artifacts();
        // Legacy single recipes retain their existing attestation contract.
        // An optional map, when present, must never introduce unreviewed roots.
        if package.stack_recipe().is_some() || !review.artifact_reviews.is_empty() {
            if review.artifact_reviews.len() != artifacts.len() {
                return Err(EngineError::Invalid(
                    "catalog requires exactly one review per declared artifact".into(),
                ));
            }
            for artifact in artifacts {
                let attestation = review
                    .artifact_reviews
                    .get(artifact.reference.as_str())
                    .ok_or_else(|| {
                        EngineError::Invalid(
                            "catalog artifact lacks an exact review attestation".into(),
                        )
                    })?;
                if attestation.digest != artifact.digest
                    || attestation.license_review.trim().is_empty()
                    || attestation.android_evidence.trim().is_empty()
                {
                    return Err(EngineError::Invalid(
                        "catalog artifact lacks matching Android and license review attestations"
                            .into(),
                    ));
                }
                public_url(&artifact.download_url)?;
            }
        }
    }
    Ok(catalog)
}

/// Useful for adapters that receive a bounded stream rather than a byte vector.
/// Reads one extra byte to distinguish an exact-size document from truncation.
pub fn read_bounded(
    mut reader: impl Read,
    maximum: usize,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 16 * 1024];
    loop {
        control.check()?;
        let available = maximum
            .saturating_sub(bytes.len())
            .saturating_add(1)
            .min(chunk.len());
        let count = reader.read(&mut chunk[..available]);
        control.check()?;
        let count = count?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(count) > maximum {
            return Err(EngineError::Invalid(
                "catalog response exceeds limit".into(),
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}
