use acropolis_common::{
    Era, GenesisDelegate, HeavyDelegate, PoolId, crypto::keyhash_224, queries::blocks::BlockIssuer,
};
use anyhow::{Result, bail};
use pallas_primitives::byron::BlockSig::DlgSig;
use pallas_traverse::{Era as PallasEra, MultiEraBlock, MultiEraHeader};
use std::collections::HashMap;

pub fn map_to_block_issuer(
    header: &MultiEraHeader,
    byron_heavy_delegates: &HashMap<PoolId, HeavyDelegate>,
    shelley_genesis_delegates: &HashMap<PoolId, GenesisDelegate>,
) -> Option<BlockIssuer> {
    match header.issuer_vkey() {
        Some(vkey) => match header {
            MultiEraHeader::ShelleyCompatible(_) => {
                let digest = keyhash_224(vkey);
                if let Some(issuer) = shelley_genesis_delegates
                    .values()
                    .find(|v| v.delegate == digest)
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

pub fn map_to_block_era(block: &MultiEraBlock) -> Result<Era> {
    Ok(match block.era() {
        PallasEra::Byron => Era::Byron,
        PallasEra::Shelley => Era::Shelley,
        PallasEra::Allegra => Era::Allegra,
        PallasEra::Mary => Era::Mary,
        PallasEra::Alonzo => Era::Alonzo,
        PallasEra::Babbage => Era::Babbage,
        PallasEra::Conway => Era::Conway,
        x => bail!(
            "Block slot {}, number {} has impossible era: {x:?}",
            block.slot(),
            block.number()
        ),
    })
}
