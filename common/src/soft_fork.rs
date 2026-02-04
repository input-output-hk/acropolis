use crate::protocol_params::ProtocolVersion;

pub fn should_check_metadata(protocol_version: &ProtocolVersion) -> bool {
    protocol_version.cmp(&ProtocolVersion { major: 2, minor: 0 }) == std::cmp::Ordering::Greater
}

pub fn should_restrict_pool_metadata_hash(protocol_version: &ProtocolVersion) -> bool {
    protocol_version.cmp(&ProtocolVersion { major: 4, minor: 0 }) == std::cmp::Ordering::Greater
}
