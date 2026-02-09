use crate::protocol_params::ProtocolVersion;

/// We need to check transaction's auxiliary data since this soft-fork
/// Which was activated after PV 2.0
/// 
/// This checks metadata is correctly formed and their size limit.
/// String and Bytes must be less than or equal to 64 bytes.
pub fn should_check_metadata(protocol_version: &ProtocolVersion) -> bool {
    protocol_version > &ProtocolVersion { major: 2, minor: 0 }
}

/// We need to restrict pool metadata hash since this soft-fork
/// Which was activated after PV 4.0
/// 
/// This checks pool metadata hash is less than or equal to 32bytes.
pub fn should_restrict_pool_metadata_hash(protocol_version: &ProtocolVersion) -> bool {
    protocol_version > &ProtocolVersion { major: 4, minor: 0 }
}
