fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("indexer_descriptor.bin"))
        .compile_protos(&["proto/indexer.proto"], &["proto"])?;
    Ok(())
}
