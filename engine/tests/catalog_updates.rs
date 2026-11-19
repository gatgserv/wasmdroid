//! Definitions only until the owner-authorized integrated acceptance phase.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::{io::Cursor, sync::Arc};
use wasmdroid_engine::{
    EngineError,
    catalog_updates::{
        ArtifactReview, CatalogFreshness, CatalogKey, CatalogManifest, CatalogUpdateConfig,
        CatalogUpdates, PackageReview, SignedCatalogIndex, read_bounded,
    },
    operation::{OperationControl, OperationState},
    session::EngineSession,
    store::Digest,
};

const NOW: u64 = 1_800_000_000;
const DOMAIN: &[u8] = b"WasmDroid curated catalog index v1\0";

fn key() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[17; 32]).unwrap()
}
fn config() -> CatalogUpdateConfig {
    CatalogUpdateConfig {
        channel: "stable".into(),
        index_url: "https://catalog.example.invalid/index.json".into(),
        trusted_keys: vec![CatalogKey {
            id: "release-1".into(),
            public_key: STANDARD.encode(key().public_key().as_ref()),
        }],
        minimum_sequence: 1,
    }
}
fn catalog() -> Vec<u8> {
    br#"{"schema_version":1,"packages":[]}"#.to_vec()
}
fn manifest(sequence: u64, bytes: &[u8]) -> CatalogManifest {
    CatalogManifest {
        schema_version: 1,
        channel: "stable".into(),
        sequence,
        issued_at_unix_seconds: NOW - 1,
        expires_at_unix_seconds: NOW + 3600,
        catalog_url: "https://catalog.example.invalid/catalog.json".into(),
        catalog_bytes: bytes.len() as u64,
        catalog_digest: Digest::of(bytes),
        package_reviews: vec![],
    }
}
fn signed_with(manifest: &CatalogManifest, domain: &[u8]) -> Vec<u8> {
    let payload = serde_json::to_vec(manifest).unwrap();
    let mut message = domain.to_vec();
    message.extend_from_slice(&payload);
    serde_json::to_vec(&SignedCatalogIndex {
        schema_version: 1,
        key_id: "release-1".into(),
        payload: STANDARD.encode(payload),
        signature: STANDARD.encode(key().sign(&message).as_ref()),
    })
    .unwrap()
}
fn signed(manifest: &CatalogManifest) -> Vec<u8> {
    signed_with(manifest, DOMAIN)
}
fn token() -> Arc<OperationControl> {
    Arc::new(OperationControl::default())
}
fn refresh(
    updates: &mut CatalogUpdates,
    manifest: &CatalogManifest,
    bytes: &[u8],
) -> wasmdroid_engine::Result<()> {
    let index = signed(manifest);
    updates
        .refresh_with(NOW, &token(), |url, _, _| {
            Ok(if url.ends_with("index.json") {
                index.clone()
            } else {
                bytes.to_vec()
            })
        })
        .map(|_| ())
}

#[test]
fn trust_requires_explicit_keys_and_public_credential_free_metadata() {
    let mut policy = config();
    policy.trusted_keys.clear();
    assert!(matches!(
        policy.validate(),
        Err(EngineError::Unsupported(_))
    ));
    for url in [
        "http://catalog.example.invalid/index",
        "https://localhost/index",
        "https://127.0.0.1/index",
        "https://user:pass@example.invalid/index",
        "https://example.invalid/index?token=secret",
        "https://example.invalid/index#fragment",
    ] {
        let mut policy = config();
        policy.index_url = url.into();
        assert!(policy.validate().is_err());
    }
    let mut policy = config();
    policy.trusted_keys.push(policy.trusted_keys[0].clone());
    assert!(policy.validate().is_err());
}

#[test]
fn signed_snapshot_survives_reopen_with_truthful_expiry_and_clock_status() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    refresh(&mut updates, &manifest(1, &bytes), &bytes).unwrap();
    drop(updates);
    let updates = CatalogUpdates::open(&session, config()).unwrap();
    assert_eq!(
        updates.snapshot(NOW).unwrap().unwrap().freshness,
        CatalogFreshness::Current
    );
    assert_eq!(
        updates.snapshot(NOW + 3600).unwrap().unwrap().freshness,
        CatalogFreshness::Expired
    );
    assert_eq!(
        updates.snapshot(NOW - 1).unwrap().unwrap().freshness,
        CatalogFreshness::ClockBeforeAcceptance
    );
}

#[test]
fn rollback_and_equivocation_preserve_last_good_snapshot_across_reopen() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let bytes = catalog();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    refresh(&mut updates, &manifest(7, &bytes), &bytes).unwrap();
    assert!(matches!(
        refresh(&mut updates, &manifest(6, &bytes), &bytes),
        Err(EngineError::Conflict(_))
    ));
    let mut changed = manifest(7, &bytes);
    changed.expires_at_unix_seconds += 1;
    assert!(matches!(
        refresh(&mut updates, &changed, &bytes),
        Err(EngineError::Conflict(_))
    ));
    // Exact replay is harmless but cannot extend signed expiration.
    refresh(&mut updates, &manifest(7, &bytes), &bytes).unwrap();
    drop(updates);
    let updates = CatalogUpdates::open(&session, config()).unwrap();
    assert_eq!(updates.snapshot(NOW).unwrap().unwrap().manifest.sequence, 7);
}

#[test]
fn signature_tampering_untrusted_keys_and_cross_protocol_signatures_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    let good = signed(&manifest(1, &bytes));
    let mut changed_payload: serde_json::Value = serde_json::from_slice(&good).unwrap();
    changed_payload["payload"] = STANDARD
        .encode(serde_json::to_vec(&manifest(2, &bytes)).unwrap())
        .into();
    let mut unknown_key: serde_json::Value = serde_json::from_slice(&good).unwrap();
    unknown_key["key_id"] = "attacker".into();
    let mut unknown_field: serde_json::Value = serde_json::from_slice(&good).unwrap();
    unknown_field["ignored"] = true.into();
    for index in [
        serde_json::to_vec(&changed_payload).unwrap(),
        serde_json::to_vec(&unknown_key).unwrap(),
        serde_json::to_vec(&unknown_field).unwrap(),
        signed_with(&manifest(1, &bytes), b""),
    ] {
        assert!(
            updates
                .refresh_with(NOW, &token(), |_, _, _| Ok(index.clone()))
                .is_err()
        );
        assert!(updates.snapshot(NOW).unwrap().is_none());
    }
}

#[test]
fn channel_floor_future_expired_unknown_schema_and_size_limits_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut policy = config();
    policy.minimum_sequence = 5;
    let mut updates = CatalogUpdates::open(&session, policy).unwrap();
    let bytes = catalog();
    let base = manifest(5, &bytes);
    let mut wrong_channel = base.clone();
    wrong_channel.channel = "preview".into();
    let mut future = base.clone();
    future.issued_at_unix_seconds = NOW + 1;
    let mut expired = base.clone();
    expired.expires_at_unix_seconds = NOW;
    let mut unknown = base.clone();
    unknown.schema_version = 2;
    let mut oversized = base.clone();
    oversized.catalog_bytes = 4 * 1024 * 1024 + 1;
    for candidate in [
        manifest(4, &bytes),
        wrong_channel,
        future,
        expired,
        unknown,
        oversized,
    ] {
        assert!(refresh(&mut updates, &candidate, &bytes).is_err());
    }
    assert!(updates.snapshot(NOW).unwrap().is_none());
}

#[test]
fn corruption_and_fetch_failure_do_not_replace_accepted_metadata() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    refresh(&mut updates, &manifest(1, &bytes), &bytes).unwrap();
    let next = manifest(2, &bytes);
    let mut corrupt = bytes.clone();
    corrupt[0] ^= 1;
    assert!(matches!(
        refresh(&mut updates, &next, &corrupt),
        Err(EngineError::DigestMismatch { .. })
    ));
    assert!(refresh(&mut updates, &next, &bytes[..bytes.len() - 1]).is_err());
    assert!(
        updates
            .refresh_with(NOW, &token(), |_, _, _| Err(EngineError::Network(
                "fixture offline".into()
            )))
            .is_err()
    );
    assert_eq!(updates.snapshot(NOW).unwrap().unwrap().manifest.sequence, 1);
}

#[test]
fn cancelled_header_or_body_phase_preserves_snapshot_and_operation_state() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    refresh(&mut updates, &manifest(1, &bytes), &bytes).unwrap();
    for phase in 1..=2 {
        let control = token();
        let mut calls = 0;
        let next = signed(&manifest(2, &bytes));
        assert!(matches!(
            updates.refresh_with(NOW, &control, |_, _, token| {
                calls += 1;
                if calls == phase {
                    token.cancel();
                }
                Ok(if calls == 1 {
                    next.clone()
                } else {
                    bytes.clone()
                })
            }),
            Err(EngineError::Cancelled)
        ));
        assert_eq!(control.state(), OperationState::Cancelled);
        assert_eq!(updates.snapshot(NOW).unwrap().unwrap().manifest.sequence, 1);
    }
    let control = token();
    control.cancel();
    assert!(matches!(
        updates.refresh_with(NOW, &control, |_, _, _| panic!("no fetch after precancel")),
        Err(EngineError::Cancelled)
    ));
}

#[test]
fn committed_update_refuses_late_cancel_and_reuse() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    let index = signed(&manifest(1, &bytes));
    let control = token();
    updates
        .refresh_with(NOW, &control, |url, _, _| {
            Ok(if url.ends_with("index.json") {
                index.clone()
            } else {
                bytes.clone()
            })
        })
        .unwrap();
    assert_eq!(control.state(), OperationState::Committed);
    assert!(!control.cancel());
    assert!(matches!(
        updates.refresh_with(NOW, &control, |_, _, _| panic!("reused token")),
        Err(EngineError::Conflict(_))
    ));
}

fn reviewed_catalog(verified: bool) -> (Vec<u8>, PackageReview) {
    let digest = Digest::of(b"real Android evidence is still required");
    let evidence = "docs/verification/example-only.md";
    let bytes = serde_json::to_vec(&serde_json::json!({"schema_version":1,"packages":[{
        "id":"example", "name":"Example", "description":"Fixture only", "publisher":"example",
        "digest":digest, "download_url":"https://packages.example.invalid/example.webc",
        "android_verified":verified,"evidence":evidence,
        "recipe":{"artifact_ref":"wasmer.io/example/site@1.0.0","command":"serve"}
    }]}))
    .unwrap();
    (
        bytes,
        PackageReview {
            package_id: "example".into(),
            artifact_digest: digest,
            license_review: "docs/licenses/example-only.md".into(),
            android_evidence: evidence.into(),
            media_review: None,
            artifact_reviews: Default::default(),
        },
    )
}

fn reviewed_stack_catalog() -> (Vec<u8>, PackageReview) {
    let web = Digest::of(b"web fixture only");
    let database = Digest::of(b"database fixture only");
    let bytes = serde_json::to_vec(&serde_json::json!({"schema_version":2,"packages":[{
        "id":"example","name":"Example","description":"Fixture, not accepted recipe",
        "publisher":"example","digest":web,
        "download_url":"https://packages.example.invalid/web.webc",
        "android_verified":true,"evidence":"fixture-stack-android.md",
        "recipe":{"type":"stack","primary_service":"web",
            "document":"services:\n  web:\n    image: wasmer.io/example/web@1.0.0\n    ports: ['8080']\n    depends_on: [database]\n  database:\n    image: wasmer.io/example/database@2.0.0\n",
            "artifacts":[
                {"reference":"wasmer.io/example/web@1.0.0","digest":web,
                 "download_url":"https://packages.example.invalid/web.webc"},
                {"reference":"wasmer.io/example/database@2.0.0","digest":database,
                 "download_url":"https://packages.example.invalid/database.webc"}]},
        "web":{"instance_port":8080,"path":"/"}
    }]})).unwrap();
    let mut review = PackageReview {
        package_id: "example".into(),
        artifact_digest: web.clone(),
        license_review: "fixture-primary-license.md".into(),
        android_evidence: "fixture-stack-android.md".into(),
        media_review: None,
        artifact_reviews: Default::default(),
    };
    for (reference, digest) in [
        ("wasmer.io/example/web@1.0.0", web),
        ("wasmer.io/example/database@2.0.0", database),
    ] {
        review.artifact_reviews.insert(
            reference.into(),
            ArtifactReview {
                digest,
                license_review: "fixture-artifact-license.md".into(),
                android_evidence: "fixture-artifact-android.md".into(),
            },
        );
    }
    (bytes, review)
}

#[test]
fn signed_stack_requires_exact_attestations_for_every_declared_root() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let (bytes, review) = reviewed_stack_catalog();
    let mut candidate = manifest(1, &bytes);
    candidate.package_reviews = vec![review.clone()];
    refresh(&mut updates, &candidate, &bytes).unwrap();
    let mut invalid = Vec::new();
    let mut absent = review.clone();
    absent.artifact_reviews.clear();
    invalid.push(absent);
    let mut missing = review.clone();
    missing
        .artifact_reviews
        .remove("wasmer.io/example/database@2.0.0");
    invalid.push(missing);
    let mut extra = review.clone();
    extra.artifact_reviews.insert(
        "wasmer.io/example/extra@1.0.0".into(),
        review.artifact_reviews.values().next().unwrap().clone(),
    );
    invalid.push(extra);
    let mut renamed = review.clone();
    let value = renamed
        .artifact_reviews
        .remove("wasmer.io/example/database@2.0.0")
        .unwrap();
    renamed
        .artifact_reviews
        .insert("wasmer.io/example/database@2.0.1".into(), value);
    invalid.push(renamed);
    let mut mismatch = review.clone();
    mismatch
        .artifact_reviews
        .get_mut("wasmer.io/example/database@2.0.0")
        .unwrap()
        .digest = Digest::of(b"other");
    invalid.push(mismatch);
    for blank_license in [true, false] {
        let mut blank = review.clone();
        let attestation = blank
            .artifact_reviews
            .get_mut("wasmer.io/example/database@2.0.0")
            .unwrap();
        if blank_license {
            attestation.license_review = " \t".into();
        } else {
            attestation.android_evidence = " \n".into();
        }
        invalid.push(blank);
    }
    let mut primary = review;
    primary.artifact_digest = Digest::of(b"not primary");
    invalid.push(primary);
    for invalid_review in invalid {
        let mut next = manifest(2, &bytes);
        next.package_reviews = vec![invalid_review];
        assert!(refresh(&mut updates, &next, &bytes).is_err());
        assert_eq!(updates.snapshot(NOW).unwrap().unwrap().manifest.sequence, 1);
    }
}

#[test]
fn reviewed_stack_selection_retains_the_whole_recipe_and_all_pins() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let (bytes, review) = reviewed_stack_catalog();
    let mut candidate = manifest(1, &bytes);
    candidate.package_reviews = vec![review];
    refresh(&mut updates, &candidate, &bytes).unwrap();
    let selected = updates
        .package_for_install(1, &Digest::of(&bytes), "example", NOW)
        .unwrap();
    assert_eq!(selected.artifacts().len(), 2);
    assert_eq!(selected.stack_recipe().unwrap().primary_service, "web");
    assert_eq!(
        selected.artifacts()[1].digest,
        Digest::of(b"database fixture only")
    );
    let mut changed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    changed["packages"][0]["recipe"]["document"] =
        "services:\n  web:\n    image: wasmer.io/example/web@1.0.0\n".into();
    let changed = serde_json::to_vec(&changed).unwrap();
    // Whole-catalog digest still commits document + every root; root pins alone
    // do not authorize a changed service graph under the same selection.
    assert!(
        updates
            .package_for_install(1, &Digest::of(&changed), "example", NOW)
            .is_err()
    );
}

#[test]
fn optional_single_artifact_map_preserves_legacy_bytes_and_rejects_wrong_maps() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let (bytes, mut review) = reviewed_catalog(true);
    assert!(
        serde_json::to_value(&review)
            .unwrap()
            .get("artifact_reviews")
            .is_none()
    );
    let mut candidate = manifest(1, &bytes);
    candidate.package_reviews = vec![review.clone()];
    refresh(&mut updates, &candidate, &bytes).unwrap();
    review.artifact_reviews.insert(
        "wasmer.io/example/site@1.0.0".into(),
        ArtifactReview {
            digest: review.artifact_digest.clone(),
            license_review: "fixture-license.md".into(),
            android_evidence: "fixture-android.md".into(),
        },
    );
    let mut candidate = manifest(2, &bytes);
    candidate.package_reviews = vec![review.clone()];
    refresh(&mut updates, &candidate, &bytes).unwrap();
    review
        .artifact_reviews
        .get_mut("wasmer.io/example/site@1.0.0")
        .unwrap()
        .digest = Digest::of(b"wrong");
    let mut candidate = manifest(3, &bytes);
    candidate.package_reviews = vec![review];
    assert!(refresh(&mut updates, &candidate, &bytes).is_err());
    assert_eq!(updates.snapshot(NOW).unwrap().unwrap().manifest.sequence, 2);
}

#[test]
fn duplicate_artifact_attestations_cannot_be_silently_overwritten() {
    let (_, review) = reviewed_stack_catalog();
    let mut value = serde_json::to_value(&review).unwrap();
    let entries = value
        .as_object_mut()
        .unwrap()
        .remove("artifact_reviews")
        .unwrap();
    let entry = serde_json::to_string(&entries["wasmer.io/example/web@1.0.0"]).unwrap();
    let prefix = serde_json::to_string(&value).unwrap();
    let raw = format!(
        "{},\"artifact_reviews\":{{\"wasmer.io/example/web@1.0.0\":{entry},\"wasmer.io/example/web@1.0.0\":{entry}}}}}",
        prefix.strip_suffix('}').unwrap()
    );
    assert!(serde_json::from_str::<PackageReview>(&raw).is_err());
}

#[test]
fn remote_catalog_requires_exact_reviewed_digest_and_android_evidence() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let (bytes, review) = reviewed_catalog(true);
    let mut candidate = manifest(1, &bytes);
    assert!(refresh(&mut updates, &candidate, &bytes).is_err());
    candidate.package_reviews = vec![review.clone()];
    candidate.package_reviews[0].artifact_digest = Digest::of(b"wrong artifact");
    assert!(refresh(&mut updates, &candidate, &bytes).is_err());
    candidate.package_reviews[0] = review.clone();
    candidate.package_reviews[0].android_evidence = "other.md".into();
    assert!(refresh(&mut updates, &candidate, &bytes).is_err());
    candidate.package_reviews[0] = review;
    refresh(&mut updates, &candidate, &bytes).unwrap();
    let (unverified, review) = reviewed_catalog(false);
    let mut candidate = manifest(2, &unverified);
    candidate.package_reviews = vec![review];
    assert!(refresh(&mut updates, &candidate, &unverified).is_err());
    assert_eq!(updates.snapshot(NOW).unwrap().unwrap().manifest.sequence, 1);
}

#[test]
fn installation_selection_is_bound_to_current_sequence_digest_and_freshness() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let (bytes, review) = reviewed_catalog(true);
    let mut candidate = manifest(1, &bytes);
    candidate.package_reviews = vec![review];
    refresh(&mut updates, &candidate, &bytes).unwrap();
    assert_eq!(
        updates
            .package_for_install(1, &Digest::of(&bytes), "example", NOW)
            .unwrap()
            .id,
        "example"
    );
    for (sequence, digest, now) in [
        (2, Digest::of(&bytes), NOW),
        (1, Digest::of(b"wrong"), NOW),
        (1, Digest::of(&bytes), NOW + 3600),
        (1, Digest::of(&bytes), NOW - 1),
    ] {
        assert!(matches!(
            updates.package_for_install(sequence, &digest, "example", now),
            Err(EngineError::Conflict(_))
        ));
    }
    refresh(&mut updates, &manifest(2, &catalog()), &catalog()).unwrap();
    assert!(
        updates
            .package_for_install(1, &Digest::of(&bytes), "example", NOW)
            .is_err()
    );
}

#[test]
fn changed_trust_does_not_silently_adopt_old_metadata_and_clock_rollback_cannot_refresh() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    refresh(&mut updates, &manifest(1, &bytes), &bytes).unwrap();
    assert!(matches!(
        updates.refresh_with(NOW - 1, &token(), |_, _, _| panic!(
            "clock rollback must not fetch"
        )),
        Err(EngineError::Conflict(_))
    ));
    drop(updates);
    let mut policy = config();
    policy.trusted_keys[0].public_key = STANDARD.encode(
        Ed25519KeyPair::from_seed_unchecked(&[19; 32])
            .unwrap()
            .public_key()
            .as_ref(),
    );
    let updates = CatalogUpdates::open(&session, policy).unwrap();
    assert!(updates.snapshot(NOW).is_err());
}

#[test]
fn updater_retains_engine_ownership_after_parent_session_drop() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let updates = CatalogUpdates::open(&session, config()).unwrap();
    drop(session);
    assert!(matches!(
        EngineSession::open(root.path()),
        Err(EngineError::Conflict(_))
    ));
    drop(updates);
    assert!(EngineSession::open(root.path()).is_ok());
}

#[test]
fn bounded_stream_rejects_extra_bytes_and_preserves_cancellation() {
    assert_eq!(
        read_bounded(Cursor::new(b"1234"), 4, &OperationControl::default()).unwrap(),
        b"1234"
    );
    assert!(read_bounded(Cursor::new(b"12345"), 4, &OperationControl::default()).is_err());
    let control = OperationControl::default();
    control.cancel();
    assert!(matches!(
        read_bounded(Cursor::new(b"1234"), 4, &control),
        Err(EngineError::Cancelled)
    ));
}

#[test]
fn concurrent_updater_cannot_overwrite_an_inflight_accepted_sequence() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut first = CatalogUpdates::open(&session, config()).unwrap();
    let mut second = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    let index = signed(&manifest(3, &bytes));
    first
        .refresh_with(NOW, &token(), |url, _, _| {
            assert!(matches!(
                second.refresh_with(NOW, &token(), |_, _, _| panic!(
                    "competing writer cannot fetch"
                )),
                Err(EngineError::Conflict(_))
            ));
            Ok(if url.ends_with("index.json") {
                index.clone()
            } else {
                bytes.clone()
            })
        })
        .unwrap();
    assert!(matches!(
        refresh(&mut second, &manifest(2, &bytes), &bytes),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(second.snapshot(NOW).unwrap().unwrap().manifest.sequence, 3);
}

#[test]
fn sqlite_publication_failure_retains_the_previous_complete_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    refresh(&mut updates, &manifest(1, &bytes), &bytes).unwrap();
    let db = rusqlite::Connection::open(session.root().join("catalog-updates.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fixture_failure BEFORE UPDATE ON catalog_snapshots BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;").unwrap();
    let control = token();
    let index = signed(&manifest(2, &bytes));
    assert!(
        updates
            .refresh_with(NOW, &control, |url, _, _| Ok(
                if url.ends_with("index.json") {
                    index.clone()
                } else {
                    bytes.clone()
                }
            ))
            .is_err()
    );
    assert_eq!(control.state(), OperationState::Failed);
    assert_eq!(updates.snapshot(NOW).unwrap().unwrap().manifest.sequence, 1);
}

#[test]
fn raising_the_shipped_floor_preserves_rollback_state_and_allows_a_newer_refresh() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = CatalogUpdates::open(&session, config()).unwrap();
    let bytes = catalog();
    refresh(&mut updates, &manifest(3, &bytes), &bytes).unwrap();
    drop(updates);
    let mut policy = config();
    policy.minimum_sequence = 5;
    let mut updates = CatalogUpdates::open(&session, policy).unwrap();
    assert_eq!(
        updates.snapshot(NOW).unwrap().unwrap().freshness,
        CatalogFreshness::BelowMinimumSequence
    );
    assert!(refresh(&mut updates, &manifest(4, &bytes), &bytes).is_err());
    refresh(&mut updates, &manifest(5, &bytes), &bytes).unwrap();
    assert_eq!(
        updates.snapshot(NOW).unwrap().unwrap().freshness,
        CatalogFreshness::Current
    );
}
