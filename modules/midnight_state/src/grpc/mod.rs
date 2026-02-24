pub mod server;
mod service;

pub mod midnight_state_proto {
    tonic::include_proto!("midnight_state");

    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("midnight_descriptor");
}
