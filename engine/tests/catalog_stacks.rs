//! Hermetic structural contracts only: no packages, databases or Android claims.
use serde_json::{Value, json};
use wasmdroid_engine::{
    api::InstanceSpec,
    catalog::{Catalog, CatalogRecipe},
    stacks::catalog::{primary_artifact, primary_ports, validate_recipe},
    store::Digest,
};

const WEB: &str = "wasmer.io/example/web@1.2.3";
const DB: &str = "wasmer.io/example/database@2.0.0";

fn document() -> String {
    format!(
        "services:\n  web:\n    image: {WEB}\n    command: [serve, /site]\n    ports: ['8080']\n    depends_on: [database]\n  database:\n    image: {DB}\n    command: [database]\n    volumes: ['data:/data']\nvolumes:\n  data: {{}}\n"
    )
}

fn fixture() -> Value {
    json!({"schema_version":2,"packages":[{
        "id":"example","name":"Example","description":"Unverified test fixture only",
        "publisher":"example","digest":Digest::of(b"web fixture"),
        "download_url":"https://packages.example.invalid/web.webc",
        "android_verified":false,"evidence":null,
        "recipe":{"type":"stack","document":document(),"primary_service":"web",
            "artifacts":[
                {"reference":WEB,"digest":Digest::of(b"web fixture"),
                 "download_url":"https://packages.example.invalid/web.webc"},
                {"reference":DB,"digest":Digest::of(b"database fixture"),
                 "download_url":"https://packages.example.invalid/database.webc"}
            ]},
        "web":{"instance_port":8080,"path":"/"}
    }]})
}

fn parse(value: &Value) -> wasmdroid_engine::Result<Catalog> {
    Catalog::parse(&serde_json::to_string(value).unwrap())
}

#[test]
fn encoded_seed_entries_must_fit_the_actual_install_boundary_before_acceptance() {
    let mut value = fixture();
    let text = "\0".repeat(64 * 1024);
    value["packages"][0]["recipe"]["assets"] = json!([{"volume":"data","path":"seed.bin","text":text,"size_bytes":64*1024,"digest":Digest::of(text.as_bytes())}]);
    let error = parse(&value).unwrap_err();
    assert!(
        matches!(error,wasmdroid_engine::EngineError::Invalid(reason) if reason=="catalog entry exceeds IPC budget")
    );
    let text = "seed";
    value["packages"][0]["recipe"]["assets"] = json!([{"volume":"data","path":"seed.txt","text":text,"size_bytes":4,"digest":Digest::of(text.as_bytes())}]);
    assert!(parse(&value).is_ok());
}

#[test]
fn legacy_single_recipe_keeps_its_wire_shape_and_schema_two_accepts_it() {
    let spec: InstanceSpec = serde_json::from_value(json!({
        "artifact_ref":WEB,"command":"serve","ports":[
            {"host_port":0,"instance_port":8080,"protocol":"tcp"}]
    }))
    .unwrap();
    let expected = serde_json::to_value(&spec).unwrap();
    for version in [1, 2] {
        let mut value = fixture();
        value["schema_version"] = version.into();
        value["packages"][0]["recipe"] = expected.clone();
        let catalog = parse(&value).unwrap();
        let package = &catalog.packages[0];
        assert!(package.single_recipe().is_some());
        assert!(package.stack_recipe().is_none());
        assert_eq!(serde_json::to_value(&package.recipe).unwrap(), expected);
        assert_eq!(package.artifacts().len(), 1);
        assert_eq!(package.artifacts()[0].reference.as_str(), WEB);
        assert_eq!(package.artifacts()[0].digest, package.digest);
    }
}

#[test]
fn legacy_catalog_and_unknown_recipe_shapes_cannot_admit_a_stack() {
    let mut value = fixture();
    value["schema_version"] = 1.into();
    assert!(parse(&value).is_err());
    value["schema_version"] = 4.into();
    assert!(parse(&value).is_err());
    value["schema_version"] = 2.into();
    value["packages"][0]["recipe"]["type"] = "compose".into();
    assert!(parse(&value).is_err());
    value["packages"][0]["recipe"]["type"] = "stack".into();
    value["packages"][0]["recipe"]["host_authority"] = true.into();
    assert!(parse(&value).is_err());
}

#[test]
fn complete_stack_validates_without_database_or_installed_package_authority() {
    let catalog = parse(&fixture()).unwrap();
    let package = &catalog.packages[0];
    assert!(package.single_recipe().is_none());
    let stack = package.stack_recipe().unwrap();
    validate_recipe(stack).unwrap();
    assert_eq!(package.artifacts().len(), 2);
    assert_eq!(primary_artifact(stack).unwrap().reference.as_str(), WEB);
    assert_eq!(primary_ports(stack).unwrap()[0].instance_port, 8080);
    // A declared reference may be reused by several services, with one pin.
    let mut value = fixture();
    value["packages"][0]["recipe"]["document"] = document().replace(DB, WEB).into();
    value["packages"][0]["recipe"]["artifacts"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_eq!(parse(&value).unwrap().packages[0].artifacts().len(), 1);
}

#[test]
fn primary_service_artifact_and_web_port_are_exactly_bound() {
    let mut value = fixture();
    value["packages"][0]["recipe"]["primary_service"] = "absent".into();
    assert!(parse(&value).is_err());
    let mut value = fixture();
    value["packages"][0]["digest"] = json!(Digest::of(b"database fixture"));
    assert!(parse(&value).is_err());
    let mut value = fixture();
    value["packages"][0]["download_url"] = "https://packages.example.invalid/other.webc".into();
    assert!(parse(&value).is_err());
    for path in ["//other.invalid/", "\\private", "/bad\npath"] {
        let mut value = fixture();
        value["packages"][0]["web"]["path"] = path.into();
        assert!(parse(&value).is_err());
    }
    let mut value = fixture();
    value["packages"][0]["recipe"]["document"] = document()
        .replace("ports: ['8080']", "ports: ['8081']")
        .replace(
            "command: [database]",
            "command: [database]\n    ports: ['8080']",
        )
        .into();
    // Another service's matching port cannot authorize the primary web target.
    assert!(parse(&value).is_err());
}

#[test]
fn roots_are_an_exact_unique_inventory_with_immutable_public_versions() {
    let mut value = fixture();
    value["packages"][0]["recipe"]["artifacts"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(parse(&value).is_err());
    let mut value = fixture();
    let extra = json!({"reference":"wasmer.io/example/unused@1.0.0",
        "digest":Digest::of(b"unused"),"download_url":"https://packages.example.invalid/unused"});
    value["packages"][0]["recipe"]["artifacts"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    assert!(parse(&value).is_err());
    let mut value = fixture();
    let duplicate = value["packages"][0]["recipe"]["artifacts"][0].clone();
    value["packages"][0]["recipe"]["artifacts"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(parse(&value).is_err());
    for reference in [
        "wasmer.io/example/web",
        "wasmer.io/example/web@latest",
        "wasmer.io/example/web@^1.2.3",
        "wasmer.io/example/web@1.2",
        "wasmer.io/example/web@01.2.3",
        "wasmer.io/extra/example/web@1.2.3",
        "local.wasmdroid/sha256/abcdef",
        "private.example/web@1.2.3",
    ] {
        let mut value = fixture();
        value["packages"][0]["recipe"]["artifacts"][0]["reference"] = reference.into();
        value["packages"][0]["recipe"]["document"] = document().replace(WEB, reference).into();
        assert!(parse(&value).is_err(), "unexpected reference accepted");
    }
}

#[test]
fn stack_root_urls_cannot_carry_private_or_non_https_authority() {
    for url in [
        "http://packages.example.invalid/a",
        "https://localhost/a",
        "https://127.0.0.1/a",
        "https://user:password@packages.example.invalid/a",
        "https://packages.example.invalid/a?token=private",
        "https://packages.example.invalid/a#secret",
    ] {
        let mut value = fixture();
        value["packages"][0]["recipe"]["artifacts"][1]["download_url"] = url.into();
        assert!(parse(&value).is_err());
    }
}

#[test]
fn dependencies_are_checked_before_any_download() {
    for input in [
        document().replace("depends_on: [database]", "depends_on: [absent]"),
        document().replace("depends_on: [database]", "depends_on: [database, database]"),
        document().replace(
            "command: [database]",
            "command: [database]\n    depends_on: [web]",
        ),
        document().replace(
            "depends_on: [database]",
            "depends_on: {database: {condition: service_healthy}}",
        ),
    ] {
        let mut value = fixture();
        value["packages"][0]["recipe"]["document"] = input.into();
        assert!(parse(&value).is_err());
    }
}

#[test]
fn catalog_documents_cannot_grant_external_or_host_data_authority() {
    for input in [
        document().replace("data: {}", "data: {external: true}"),
        document().replace("data: {}", "data: {external: true, name: existing_private}"),
        document().replace("data: {}", "data: {name: existing_private}"),
        document().replace("data:/data", "absent:/data"),
        document().replace("data:/data", "./host:/data"),
        document().replace("data:/data", "data:/../private"),
        document().replace(
            "volumes: ['data:/data']",
            "volumes: [{type: bind, source: data, target: /data}]",
        ),
        document().replace(
            "volumes: ['data:/data']",
            "volumes: ['data:/data', 'data:/data']",
        ),
    ] {
        let mut value = fixture();
        value["packages"][0]["recipe"]["document"] = input.into();
        assert!(parse(&value).is_err());
    }
}

#[test]
fn cross_service_fixed_ports_conflict_but_automatic_allocations_do_not() {
    let mut value = fixture();
    value["packages"][0]["recipe"]["document"] = document()
        .replace("'8080'", "'9000:8080'")
        .replace(
            "command: [database]",
            "command: [database]\n    ports: ['9000:5432']",
        )
        .into();
    assert!(parse(&value).is_err());
    value["packages"][0]["recipe"]["document"] = document()
        .replace(
            "command: [database]",
            "command: [database]\n    ports: ['8080']",
        )
        .into();
    parse(&value).unwrap();
}

#[test]
fn parser_sandbox_and_neutral_spec_constraints_apply_to_every_service() {
    for input in [
        document().replace("command: [database]", "command: []"),
        document().replace(
            "command: [database]",
            "command: [database]\n    working_dir: relative",
        ),
        document().replace(
            "command: [database]",
            "command: [database]\n    environment: {PASSWORD: '${HOST_SECRET}'}",
        ),
        document().replace(
            "command: [database]",
            "command: [database]\n    network_mode: host",
        ),
        document().replace(
            "command: [database]",
            "command: [database]\n    mem_limit: 1g",
        ),
        document().replace(
            "command: [database]",
            "command: [database]\n    restart: always",
        ),
        document().replace(
            "command: [database]",
            "command: [database]\n    image: duplicate",
        ),
        document().replace("data: {}", "data: &data {}"),
        document().replace("ports: ['8080']", "ports: ['8080/udp']"),
    ] {
        let mut value = fixture();
        value["packages"][0]["recipe"]["document"] = input.into();
        assert!(parse(&value).is_err());
    }
}

#[test]
fn service_artifact_and_document_budgets_remain_bounded() {
    let mut input = document();
    for index in 0..7 {
        input = input.replace(
            "volumes:\n  data",
            &format!("  extra{index}:\n    image: {WEB}\nvolumes:\n  data"),
        );
    }
    let mut value = fixture();
    value["packages"][0]["recipe"]["document"] = input.into();
    assert!(parse(&value).is_err());
    value["packages"][0]["recipe"]["document"] = " ".repeat(128 * 1024 + 1).into();
    assert!(parse(&value).is_err());
    value = fixture();
    let extra = value["packages"][0]["recipe"]["artifacts"][1].clone();
    for _ in 0..7 {
        value["packages"][0]["recipe"]["artifacts"]
            .as_array_mut()
            .unwrap()
            .push(extra.clone());
    }
    assert!(parse(&value).is_err());
}

#[test]
fn json_compose_and_redacted_diagnostics_preserve_the_same_boundary() {
    let mut value = fixture();
    value["packages"][0]["recipe"]["document"] = serde_json::to_string(&json!({
        "services":{"web":{"image":WEB,"command":["serve"],"ports":["8080"],
            "environment":{"PRIVATE":"never-print-this"}},"database":{"image":DB}}
    }))
    .unwrap()
    .into();
    let catalog = parse(&value).unwrap();
    let recipe = &catalog.packages[0].recipe;
    assert!(matches!(recipe, CatalogRecipe::Stack(_)));
    assert!(!format!("{catalog:?}").contains("never-print-this"));
    assert!(
        !format!("{:?}", catalog.packages[0].stack_recipe().unwrap()).contains("never-print-this")
    );
}
