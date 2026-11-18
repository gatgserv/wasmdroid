//! Inline framing/authentication contracts only. Hand-built fixtures are not
//! screenshots, license reviews, Android decoder or accepted runtime evidence.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use flate2::{Compression, write::ZlibEncoder};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use std::{io::Write, sync::Arc};
use wasmdroid_engine::{
    EngineError,
    catalog::Catalog,
    catalog_media::{
        CatalogImage, CatalogImageFormat, CatalogMedia, CatalogMediaProvenance, validate,
    },
    catalog_updates::{
        CatalogKey, CatalogManifest, CatalogUpdateConfig, CatalogUpdates, PackageReview,
        SignedCatalogIndex,
    },
    operation::OperationControl,
    session::EngineSession,
    store::Digest,
};

fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(body);
    let mut crc = !0u32;
    for byte in &bytes[4..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xedb8_8320
            };
        }
    }
    bytes.extend((!crc).to_be_bytes());
    bytes
}
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut header = width.to_be_bytes().to_vec();
    header.extend(height.to_be_bytes());
    header.extend([8, 0, 0, 0, 0]);
    let mut encoded = ZlibEncoder::new(Vec::new(), Compression::none());
    for _ in 0..height {
        encoded.write_all(&[0]).unwrap();
        // Stored deflate keeps marker-looking ordinary pixel data intact.
        for index in 0..width {
            encoded.write_all(&[b"acTL"[index as usize % 4]]).unwrap();
        }
    }
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(chunk(b"IHDR", &header));
    bytes.extend(chunk(b"IDAT", &encoded.finish().unwrap()));
    bytes.extend(chunk(b"IEND", &[]));
    bytes
}
fn segment(marker: u8, data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xff, marker];
    bytes.extend(((data.len() + 2) as u16).to_be_bytes());
    bytes.extend(data);
    bytes
}
fn jpeg() -> Vec<u8> {
    // One 8-bit gray pixel, quantization 1, DC 0 and AC EOB with one-bit Huffman
    // codes; the two zero entropy bits are padded with ones. No third-party art.
    let mut bytes = vec![0xff, 0xd8];
    let mut quant = vec![0];
    quant.extend([1; 64]);
    bytes.extend(segment(0xdb, &quant));
    bytes.extend(segment(0xc0, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0]));
    for class in [0, 0x10] {
        let mut table = vec![class, 1];
        table.extend([0; 15]);
        table.push(0);
        bytes.extend(segment(0xc4, &table));
    }
    bytes.extend(segment(0xda, &[1, 1, 0, 0, 63, 0]));
    bytes.extend([0x3f, 0xff, 0xd9]);
    bytes
}
fn image(bytes: Vec<u8>, format: CatalogImageFormat, width: u32, height: u32) -> CatalogImage {
    CatalogImage {
        format,
        size_bytes: bytes.len() as u32,
        digest: Digest::of(&bytes),
        width,
        height,
        data_base64: STANDARD.encode(bytes),
        alt: "First-party test pixels".into(),
        provenance: CatalogMediaProvenance {
            license: "LicenseRef-TestFixture".into(),
            attribution: "Hand-built contract fixture; not a product screenshot".into(),
            report: "docs/verification/media-fixture-only.md".into(),
        },
    }
}
fn media() -> CatalogMedia {
    CatalogMedia {
        icon: image(png(4, 1), CatalogImageFormat::Png, 4, 1),
        screenshots: vec![image(jpeg(), CatalogImageFormat::Jpeg, 1, 1)],
    }
}
fn replace_bytes(image: &mut CatalogImage, bytes: &[u8]) {
    image.size_bytes = bytes.len() as u32;
    image.digest = Digest::of(bytes);
    image.data_base64 = STANDARD.encode(bytes);
}
fn value(version: u32, verified: bool, pictures: Option<CatalogMedia>) -> Value {
    let mut package = json!({"id":"fixture","name":"Fixture","description":"Not an accepted application",
        "publisher":"fixture","digest":Digest::of(b"runtime fixture"),
        "download_url":"https://packages.example.invalid/fixture.webc", "android_verified":verified,
        "evidence":if verified { Some("docs/verification/fixture-only.md") }else{None},
        "recipe":{"artifact_ref":"wasmer.io/example/site@1.0.0","command":"serve"}});
    if let Some(pictures) = pictures {
        package["media"] = serde_json::to_value(pictures).unwrap();
    }
    json!({"schema_version":version,"packages":[package]})
}
fn parse(value: &Value) -> wasmdroid_engine::Result<Catalog> {
    Catalog::parse(&value.to_string())
}

#[test]
fn legacy_serialization_and_review_shape_omit_media_and_media_review() {
    for version in [1, 2] {
        let catalog = parse(&value(version, true, None)).unwrap();
        let roundtrip = serde_json::to_value(catalog).unwrap();
        assert!(roundtrip["packages"][0].get("media").is_none());
        assert_eq!(roundtrip["packages"][0]["recipe"]["command"], "serve");
    }
    let review = review(None);
    let mut raw = serde_json::to_value(review).unwrap();
    assert!(raw.get("media_review").is_none());
    raw["media_review"] = Value::Null;
    assert!(serde_json::from_value::<PackageReview>(raw).is_err());
}
#[test]
fn schema_three_is_explicit_and_verified_entries_require_icon_and_screenshots() {
    for version in [1, 2] {
        assert!(parse(&value(version, false, Some(media()))).is_err());
    }
    for version in [1, 2, 3] {
        let mut null = value(version, false, None);
        null["packages"][0]["media"] = Value::Null;
        assert!(parse(&null).is_err());
    }
    assert!(parse(&value(3, false, None)).is_ok());
    assert!(parse(&value(3, true, None)).is_err());
    assert!(parse(&value(3, true, Some(media()))).is_ok());
    assert!(parse(&value(4, false, Some(media()))).is_err());
    let mut pictures = media();
    pictures.screenshots.clear();
    assert!(validate(&pictures).is_err());
    pictures.screenshots = vec![media().icon; 3];
    assert!(validate(&pictures).is_err());
}
#[test]
fn schema_three_keeps_the_existing_exact_pinned_stack_contract() {
    let mut catalog = value(3, true, Some(media()));
    catalog["packages"][0]["recipe"] = json!({"type":"stack","primary_service":"web",
        "document":"services:\n  web:\n    image: wasmer.io/example/site@1.0.0\n    command: [serve]\n",
        "artifacts":[{"reference":"wasmer.io/example/site@1.0.0","digest":Digest::of(b"runtime fixture"),
            "download_url":"https://packages.example.invalid/fixture.webc"}]});
    assert!(
        parse(&catalog).unwrap().packages[0]
            .stack_recipe()
            .is_some()
    );
    catalog["packages"][0]["recipe"]["artifacts"][0]["digest"] =
        Digest::of(b"different").as_str().into();
    assert!(parse(&catalog).is_err());
}
#[test]
fn static_png_and_baseline_jpeg_are_admitted_without_exposing_raw_media_in_debug() {
    let pictures = media();
    validate(&pictures).unwrap();
    let debug = format!("{pictures:?}");
    assert!(!debug.contains(&pictures.icon.data_base64));
    assert!(!debug.contains(&pictures.icon.alt));
    let bytes = STANDARD.decode(&pictures.icon.data_base64).unwrap();
    assert_eq!(&bytes[bytes.len() - 4..], &[0xae, 0x42, 0x60, 0x82]); // Standard IEND CRC.
    assert!(bytes.windows(4).any(|window| window == b"acTL")); // Pixel data is not an APNG chunk.
}
#[test]
fn unsupported_or_unknown_media_shapes_cannot_be_ignored() {
    let baseline = value(3, false, Some(media()));
    for field in ["url", "on_load", "unknown"] {
        let mut bad = baseline.clone();
        bad["packages"][0]["media"]["icon"][field] = "not allowed".into();
        assert!(parse(&bad).is_err());
    }
    let mut bad = baseline.clone();
    bad["packages"][0]["media"]["icon"]["format"] = "svg".into();
    assert!(parse(&bad).is_err());
    let mut bad = baseline;
    bad["packages"][0]["media"]["icon"]["provenance"]["url"] = "https://example.invalid".into();
    assert!(parse(&bad).is_err());
}
#[test]
fn exact_digest_length_and_canonical_base64_are_all_required() {
    let baseline = media();
    let mut bad = baseline.clone();
    bad.icon.digest = Digest::of(b"wrong");
    assert!(validate(&bad).is_err());
    let mut bad = baseline.clone();
    bad.icon.size_bytes += 1;
    assert!(validate(&bad).is_err());
    let mut bad = baseline.clone();
    bad.icon.data_base64.push(' ');
    assert!(validate(&bad).is_err());
    let mut bad = baseline.clone();
    bad.icon.data_base64 = "A===".into();
    assert!(validate(&bad).is_err());
    let mut bad = baseline;
    bad.icon.data_base64 = "x".repeat(20 * 1024);
    assert!(validate(&bad).is_err());
}
#[test]
fn image_and_dimension_limits_are_checked_before_any_platform_decode() {
    for (width, height) in [(0, 1), (1, 0), (257, 1), (u32::MAX, u32::MAX)] {
        let mut bad = media();
        bad.icon.width = width;
        bad.icon.height = height;
        assert!(validate(&bad).is_err());
    }
    for (width, height) in [(1281, 1), (721, 721), (1000, 1000)] {
        let mut bad = media();
        bad.screenshots[0].width = width;
        bad.screenshots[0].height = height;
        assert!(validate(&bad).is_err());
    }
    let mut bad = media();
    bad.icon.width += 1;
    assert!(validate(&bad).is_err());
    let mut bad = media();
    replace_bytes(&mut bad.icon, &[0; 8 * 1024 + 1]);
    assert!(validate(&bad).is_err());
    let mut bad = media();
    replace_bytes(&mut bad.screenshots[0], &[0; 20 * 1024 + 1]);
    assert!(validate(&bad).is_err());
}
#[test]
fn alt_and_inert_provenance_require_bounded_actual_review_references() {
    for alt in [
        "".into(),
        " ".into(),
        "line\nbreak".into(),
        "x".repeat(241),
        "é".repeat(121),
    ] {
        let mut bad = media();
        bad.icon.alt = alt;
        assert!(validate(&bad).is_err());
    }
    for report in [
        "/host/path.md",
        "docs/../secret.md",
        "https://example.invalid/review.md",
        "docs//review.md",
        "docs/review.txt",
    ] {
        let mut bad = media();
        bad.icon.provenance.report = report.into();
        assert!(validate(&bad).is_err());
    }
    let mut bad = media();
    bad.icon.provenance.license.clear();
    assert!(validate(&bad).is_err());
    let mut bad = media();
    bad.icon.provenance.attribution = "x".repeat(257);
    assert!(validate(&bad).is_err());
}
#[test]
fn png_crc_length_chunk_order_and_trailing_payload_are_checked() {
    let bytes = png(4, 1);
    let mut bad = media();
    let mut corrupt = bytes.clone();
    corrupt[29] ^= 1;
    replace_bytes(&mut bad.icon, &corrupt);
    assert!(validate(&bad).is_err());
    for cut in [7, 12, 24, bytes.len() - 1] {
        let mut bad = media();
        replace_bytes(&mut bad.icon, &bytes[..cut]);
        assert!(validate(&bad).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    replace_bytes(&mut bad.icon, &extra);
    assert!(validate(&bad).is_err());
    let mut huge = bytes.clone();
    huge[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    replace_bytes(&mut bad.icon, &huge);
    assert!(validate(&bad).is_err());
    let mut repeated = bytes[..33].to_vec();
    repeated.extend(&bytes[8..33]);
    repeated.extend(&bytes[33..]);
    replace_bytes(&mut bad.icon, &repeated);
    assert!(validate(&bad).is_err());
}
#[test]
fn png_animation_and_metadata_are_rejected_at_real_chunk_boundaries() {
    for kind in [b"acTL", b"fcTL", b"fdAT", b"iCCP", b"tEXt"] {
        let original = png(4, 1);
        let mut bytes = original[..33].to_vec();
        bytes.extend(chunk(kind, b"fixture"));
        bytes.extend(&original[33..]);
        let mut bad = media();
        replace_bytes(&mut bad.icon, &bytes);
        assert!(validate(&bad).is_err());
    }
    // Framed pixel data containing the same ASCII pattern remains acceptable.
    validate(&media()).unwrap();
}
#[test]
fn jpeg_metadata_other_frames_truncation_and_marker_lengths_are_rejected() {
    let original = jpeg();
    for marker in [0xe1, 0xe2, 0xfe] {
        let mut bytes = original[..2].to_vec();
        bytes.extend(segment(marker, b"Exif fixture"));
        bytes.extend(&original[2..]);
        let mut bad = media();
        replace_bytes(&mut bad.screenshots[0], &bytes);
        assert!(validate(&bad).is_err());
    }
    for cut in [1, 4, original.len() - 1] {
        let mut bad = media();
        replace_bytes(&mut bad.screenshots[0], &original[..cut]);
        assert!(validate(&bad).is_err());
    }
    let mut progressive = original.clone();
    let sof = progressive
        .windows(2)
        .position(|pair| pair == [0xff, 0xc0])
        .unwrap();
    progressive[sof + 1] = 0xc2;
    let mut bad = media();
    replace_bytes(&mut bad.screenshots[0], &progressive);
    assert!(validate(&bad).is_err());
    let mut length = original.clone();
    length[4..6].copy_from_slice(&0u16.to_be_bytes());
    replace_bytes(&mut bad.screenshots[0], &length);
    assert!(validate(&bad).is_err());
    let mut trailing = original;
    trailing.push(0);
    replace_bytes(&mut bad.screenshots[0], &trailing);
    assert!(validate(&bad).is_err());
}
#[test]
fn image_framing_is_not_a_pixel_decompression_or_android_acceptance_claim() {
    // CRC-valid IDAT framing with intentionally non-deflate bytes can pass the
    // envelope. Android must still reject an unsuccessful actual pixel decode.
    let original = png(4, 1);
    let mut framed = original[..33].to_vec();
    framed.extend(chunk(b"IDAT", b"not deflate"));
    framed.extend(chunk(b"IEND", &[]));
    let mut pictures = media();
    replace_bytes(&mut pictures.icon, &framed);
    validate(&pictures).unwrap();
}
#[test]
fn media_shares_the_unchanged_serialized_entry_budget_with_recipe_and_text() {
    let mut catalog = value(3, false, None);
    let initial = parse(&catalog).unwrap();
    let overhead = serde_json::to_vec(&initial.packages[0]).unwrap().len();
    catalog["packages"][0]["description"] = "x".repeat(128 * 1024 - overhead - 32).into();
    assert!(parse(&catalog).is_ok());
    catalog["packages"][0]["media"] = serde_json::to_value(media()).unwrap();
    assert!(
        matches!(parse(&catalog),Err(EngineError::Invalid(reason)) if reason=="catalog entry exceeds IPC budget")
    );
}

const NOW: u64 = 1_800_000_000;
fn key() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[37; 32]).unwrap()
}
fn review(media_review: Option<&str>) -> PackageReview {
    PackageReview {
        package_id: "fixture".into(),
        artifact_digest: Digest::of(b"runtime fixture"),
        license_review: "docs/verification/license-fixture-only.md".into(),
        android_evidence: "docs/verification/fixture-only.md".into(),
        media_review: media_review.map(str::to_owned),
        artifact_reviews: Default::default(),
    }
}
fn manifest(sequence: u64, bytes: &[u8], review: PackageReview) -> CatalogManifest {
    CatalogManifest {
        schema_version: 1,
        channel: "stable".into(),
        sequence,
        issued_at_unix_seconds: NOW - 1,
        expires_at_unix_seconds: NOW + 3600,
        catalog_url: "https://catalog.example.invalid/catalog.json".into(),
        catalog_bytes: bytes.len() as u64,
        catalog_digest: Digest::of(bytes),
        package_reviews: vec![review],
    }
}
fn signed(manifest: &CatalogManifest) -> Vec<u8> {
    let payload = serde_json::to_vec(manifest).unwrap();
    let mut message = b"WasmDroid curated catalog index v1\0".to_vec();
    message.extend(&payload);
    serde_json::to_vec(&SignedCatalogIndex {
        schema_version: 1,
        key_id: "fixture".into(),
        payload: STANDARD.encode(payload),
        signature: STANDARD.encode(key().sign(&message).as_ref()),
    })
    .unwrap()
}
fn updates(session: &EngineSession) -> CatalogUpdates {
    CatalogUpdates::open(
        session,
        CatalogUpdateConfig {
            channel: "stable".into(),
            index_url: "https://catalog.example.invalid/index.json".into(),
            trusted_keys: vec![CatalogKey {
                id: "fixture".into(),
                public_key: STANDARD.encode(key().public_key().as_ref()),
            }],
            minimum_sequence: 1,
        },
    )
    .unwrap()
}
fn refresh(
    updates: &mut CatalogUpdates,
    manifest: &CatalogManifest,
    bytes: &[u8],
) -> wasmdroid_engine::Result<()> {
    let index = signed(manifest);
    updates
        .refresh_with(NOW, &Arc::new(OperationControl::default()), |url, _, _| {
            Ok(if url.ends_with("index.json") {
                index.clone()
            } else {
                bytes.to_vec()
            })
        })
        .map(|_| ())
}
#[test]
fn signed_media_requires_explicit_review_and_rejection_preserves_last_good() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = updates(&session);
    let bytes = serde_json::to_vec(&value(3, true, Some(media()))).unwrap();
    for report in [
        None,
        Some(""),
        Some("https://example.invalid/not-committed.md"),
    ] {
        assert!(refresh(&mut updates, &manifest(1, &bytes, review(report)), &bytes).is_err());
        assert!(updates.snapshot(NOW).unwrap().is_none());
    }
    refresh(
        &mut updates,
        &manifest(
            1,
            &bytes,
            review(Some("docs/verification/media-fixture-only.md")),
        ),
        &bytes,
    )
    .unwrap();
    assert!(refresh(&mut updates, &manifest(2, &bytes, review(None)), &bytes).is_err());
    assert_eq!(updates.snapshot(NOW).unwrap().unwrap().manifest.sequence, 1);
}
#[test]
fn media_review_cannot_attest_absent_media_or_manufacture_android_verification() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = updates(&session);
    let absent = serde_json::to_vec(&value(2, true, None)).unwrap();
    assert!(
        refresh(
            &mut updates,
            &manifest(
                1,
                &absent,
                review(Some("docs/verification/media-fixture-only.md"))
            ),
            &absent
        )
        .is_err()
    );
    let candidate = serde_json::to_vec(&value(3, false, Some(media()))).unwrap();
    assert!(
        refresh(
            &mut updates,
            &manifest(
                1,
                &candidate,
                review(Some("docs/verification/media-fixture-only.md"))
            ),
            &candidate
        )
        .is_err()
    );
    assert!(updates.snapshot(NOW).unwrap().is_none());
}
#[test]
fn signed_catalog_digest_and_selection_pin_bind_media_even_when_artifact_is_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let mut updates = updates(&session);
    let first = value(3, true, Some(media()));
    let bytes = serde_json::to_vec(&first).unwrap();
    let one = manifest(
        1,
        &bytes,
        review(Some("docs/verification/media-fixture-only.md")),
    );
    refresh(&mut updates, &one, &bytes).unwrap();
    let mut changed = first;
    changed["packages"][0]["media"]["icon"]["alt"] = "Different actual caption".into();
    let replacement = serde_json::to_vec(&changed).unwrap();
    assert!(refresh(&mut updates, &one, &replacement).is_err());
    let two = manifest(
        2,
        &replacement,
        review(Some("docs/verification/media-fixture-only.md")),
    );
    refresh(&mut updates, &two, &replacement).unwrap();
    assert_ne!(one.catalog_digest, two.catalog_digest);
    assert!(
        updates
            .package_for_install(1, &one.catalog_digest, "fixture", NOW)
            .is_err()
    );
    let selected = updates
        .package_for_install(2, &two.catalog_digest, "fixture", NOW)
        .unwrap();
    assert_eq!(selected.digest, Digest::of(b"runtime fixture"));
    assert_eq!(selected.media.unwrap().icon.alt, "Different actual caption");
}
