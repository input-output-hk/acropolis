use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let descriptor_path = out_dir.join("midnight_descriptor.bin");

    tonic_build::configure()
        .build_server(true)
        .file_descriptor_set_path(&descriptor_path)
        .compile_protos(&["proto/midnight_state.proto"], &["proto"])?;

    Ok(())
}
