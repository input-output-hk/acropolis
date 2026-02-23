fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);

    tonic_build::configure()
        .build_server(true)
        .compile_protos(&["proto/midnight_state.proto"], &["proto"])?;

    Ok(())
}
