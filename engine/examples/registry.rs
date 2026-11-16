use wasmdroid_engine::{
    api::ArtifactReference,
    installer::{InstallLimits, Installer},
    registry::{Registry, wasmer::WasmerRegistry},
    state::StateDb,
    store::ContentStore,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let registry = WasmerRegistry::new()?;
    match args.get(1).map(String::as_str) {
        Some("search") => println!(
            "{}",
            serde_json::to_string_pretty(&registry.search(
                args.get(2).ok_or("search needs a query")?,
                None,
                10
            )?)?
        ),
        Some("resolve") => {
            let resolved = registry.resolve(&ArtifactReference::parse(
                args.get(2).ok_or("resolve needs a reference")?,
            )?)?;
            println!("{}", serde_json::to_string_pretty(&resolved.artifact)?);
            println!("dependencies={}", resolved.dependencies.len());
        }
        Some("pull") => {
            let root = std::path::PathBuf::from(args.get(3).ok_or("pull needs a data directory")?);
            std::fs::create_dir_all(&root)?;
            let store = ContentStore::open(root.join("store"))?;
            let state = StateDb::open(root.join("engine.sqlite"))?;
            let installer = Installer {
                registry: &registry,
                store: &store,
                state: &state,
                limits: InstallLimits::default(),
            };
            let installed = installer.pull(
                &ArtifactReference::parse(args.get(2).ok_or("pull needs a reference")?)?,
                &mut |_| {},
            )?;
            println!("{}", serde_json::to_string_pretty(&installed)?);
        }
        _ => return Err(
            "usage: registry search <query> | resolve <reference> | pull <reference> <data-dir>"
                .into(),
        ),
    }
    Ok(())
}
