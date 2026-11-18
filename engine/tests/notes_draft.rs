//! Structural draft checks only. No public package or Android execution here.
use wasmdroid_engine::{
    EngineError, catalog::Catalog, catalog_install::prepare_stack, session::EngineSession,
    store::Digest,
};

const REFERENCE: &str = "wasmer.io/wasmer/static-web-server@2.44.0";
const PIN: &str = "sha256:1e84109da307837d8160d23a77c36403ab44f7ec0c592b1cfe4a5bd79c968e08";
const DRAFT: &str = include_str!("../../examples/catalog-seeds/notes/notes-draft.catalog.json");

#[test]
fn actual_notes_draft_is_bounded_pinned_and_uses_exact_existing_seed_bytes() {
    let catalog = Catalog::parse(DRAFT).unwrap();
    assert_eq!(catalog.schema_version, 2);
    assert_eq!(catalog.packages.len(), 1);
    let package = &catalog.packages[0];
    assert!(!package.android_verified);
    assert!(package.evidence.is_none());
    assert_eq!(package.digest.as_str(), PIN);
    let recipe = package.stack_recipe().unwrap();
    assert_eq!(recipe.primary_service, "web");
    assert_eq!(recipe.artifacts.len(), 1);
    assert_eq!(recipe.artifacts[0].reference.as_str(), REFERENCE);
    assert_eq!(recipe.artifacts[0].digest, package.digest);
    assert_eq!(recipe.artifacts[0].download_url, package.download_url);
    assert_eq!(recipe.assets.len(), 1);
    assert_eq!(recipe.assets[0].volume, "site");
    assert_eq!(recipe.assets[0].path, "index.html");
    let html = include_bytes!("../../examples/catalog-seeds/notes/index.html");
    assert_eq!(recipe.assets[0].text.as_bytes(), html);
    assert_eq!(recipe.assets[0].digest, Digest::of(html));
    let original: serde_json::Value = serde_json::from_str(include_str!(
        "../../examples/catalog-seeds/notes/assets.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::to_value(&recipe.assets).unwrap(),
        original["assets"]
    );
}

#[test]
fn draft_command_and_scoped_readonly_mount_match_the_acceptance_contract() {
    let catalog = Catalog::parse(DRAFT).unwrap();
    let package = &catalog.packages[0];
    let source: serde_json::Value =
        serde_json::from_str(&package.stack_recipe().unwrap().document).unwrap();
    assert_eq!(source["services"]["web"]["image"], REFERENCE);
    assert_eq!(
        source["services"]["web"]["command"],
        serde_json::json!([
            "webserver",
            "--host",
            "127.0.0.1",
            "--port",
            "8080",
            "--root",
            "/public",
            "--threads-multiplier",
            "1",
            "--max-blocking-threads",
            "2"
        ])
    );
    assert_eq!(
        source["services"]["web"]["ports"],
        serde_json::json!(["8080"])
    );
    assert_eq!(
        source["services"]["web"]["volumes"],
        serde_json::json!(["site:/public:ro"])
    );
    assert_eq!(source["volumes"], serde_json::json!({"site":{}}));
    assert_eq!(package.web.as_ref().unwrap().instance_port, 8080);
}

#[test]
fn unverified_draft_cannot_enter_curated_preparation_or_the_shipped_catalog() {
    let catalog = Catalog::parse(DRAFT).unwrap();
    let package = &catalog.packages[0];
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    assert!(matches!(
        prepare_stack(&session, "draft-only", package, &mut |_| {
            panic!("unverified metadata must fail before runtime validation")
        }),
        Err(EngineError::Unsupported(_))
    ));
    assert!(session.state().volumes().unwrap().is_empty());
    assert!(session.state().stack("draft-only").is_err());
    let shipped = Catalog::parse(include_str!("../../catalog/catalog.v1.json")).unwrap();
    assert!(
        shipped
            .packages
            .iter()
            .all(|candidate| candidate.id != package.id)
    );
}
