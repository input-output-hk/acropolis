use acropolis_common::{
    GenesisDelegate, HeavyDelegate, crypto::keyhash_224, queries::blocks::BlockIssuer,
};
use pallas_primitives::byron::BlockSig::DlgSig;
use pallas_traverse::MultiEraHeader;
use std::collections::HashMap;

pub fn map_to_block_issuer(
    header: &MultiEraHeader,
    byron_heavy_delegates: &HashMap<Vec<u8>, HeavyDelegate>,
    shelley_genesis_delegates: &HashMap<Vec<u8>, GenesisDelegate>,
) -> Option<BlockIssuer> {
    match header.issuer_vkey() {
        Some(vkey) => match header {
            MultiEraHeader::ShelleyCompatible(_) => {
                let digest = keyhash_224(vkey);
                if let Some(issuer) = shelley_genesis_delegates
                    .values()
                    .find(|v| v.delegate == digest.to_vec())
                    .map(|i| BlockIssuer::GenesisDelegate(i.clone()))
                {
                    Some(issuer)
                } else {
                    Some(BlockIssuer::SPO(vkey.to_vec()))
                }
            }
            _ => Some(BlockIssuer::SPO(vkey.to_vec())),
        },
        None => match header {
            MultiEraHeader::Byron(_) => match header.as_byron() {
                Some(block_head) => match &block_head.consensus_data.3 {
                    DlgSig(sig) => byron_heavy_delegates
                        .values()
                        .find(|v| v.issuer_pk == *sig.0.issuer)
                        .map(|i| BlockIssuer::HeavyDelegate(i.clone())),
                    _ => None,
                },
                None => None,
            },
            _ => None,
        },
    }
}
