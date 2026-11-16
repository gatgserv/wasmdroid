fn main() -> anyhow::Result<()> {
    print!("{}", wasmdroid_wasmer_local::run_probe()?);
    Ok(())
}
