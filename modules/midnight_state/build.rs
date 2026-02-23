fn main() {
    tonic_build::configure()
        .build_server(true)
        .compile_protos(&["proto/midnight_state.proto"], &["proto"])
        .unwrap();
}
