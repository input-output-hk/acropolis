use crate::{
    grpc::midnight_state_proto::{
        AssetCreate as AssetCreateProto, AssetSpend as AssetSpendProto,
        BridgeUtxo as BridgeUtxoProto, Deregistration as DeregistrationProto, EpochCandidate,
        Registration as RegistrationProto, UtxoId,
    },
    types::{
        AssetCreate as AssetCreateInternal, AssetSpend as AssetSpendInternal,
        BridgeAssetUtxo as BridgeAssetUtxoInternal, Deregistration as DeregistrationInternal,
        Registration as RegistrationInternal, RegistrationEvent,
    },
};
impl From<AssetCreateInternal> for AssetCreateProto {
    fn from(c: AssetCreateInternal) -> Self {
        AssetCreateProto {
            address: c.holder_address.to_bytes_key(),
            quantity: c.quantity,
            tx_hash: c.tx_hash.to_vec(),
            output_index: c.utxo_index.into(),
            block_number: c.block_number,
            block_hash: c.block_hash.to_vec(),
            tx_index: c.tx_index_in_block,
            block_timestamp_unix: c.block_timestamp,
        }
    }
}

impl From<AssetSpendInternal> for AssetSpendProto {
    fn from(c: AssetSpendInternal) -> Self {
        AssetSpendProto {
            address: c.holder_address.to_bytes_key(),
            quantity: c.quantity,
            spending_tx_hash: c.spending_tx_hash.to_vec(),
            block_number: c.block_number,
            block_hash: c.block_hash.to_vec(),
            tx_index: c.tx_index_in_block,
            utxo_tx_hash: c.utxo_tx_hash.to_vec(),
            utxo_index: c.utxo_index.into(),
            block_timestamp_unix: c.block_timestamp,
        }
    }
}

impl From<BridgeAssetUtxoInternal> for BridgeUtxoProto {
    fn from(utxo: BridgeAssetUtxoInternal) -> Self {
        BridgeUtxoProto {
            tx_hash: utxo.tx_hash.to_vec(),
            output_index: utxo.output_index.into(),
            block_number: utxo.block_number,
            block_hash: utxo.block_hash.to_vec(),
            tx_index: utxo.tx_index_in_block,
            block_timestamp_unix: utxo.block_timestamp,
            tokens_out: utxo.tokens_out,
            tokens_in: utxo.tokens_in,
            datum: utxo.datum,
        }
    }
}

impl From<RegistrationInternal> for RegistrationProto {
    fn from(c: RegistrationInternal) -> Self {
        let full_datum = c.full_datum.to_bytes().expect("datum should always be inline");

        RegistrationProto {
            full_datum,
            tx_hash: c.tx_hash.to_vec(),
            output_index: c.utxo_index.into(),
            block_number: c.block_number,
            block_hash: c.block_hash.to_vec(),
            tx_index: c.tx_index_in_block,
            block_timestamp_unix: c.block_timestamp,
        }
    }
}

impl From<DeregistrationInternal> for DeregistrationProto {
    fn from(c: DeregistrationInternal) -> Self {
        let full_datum = c.full_datum.to_bytes().expect("datum  should always be inline");

        DeregistrationProto {
            full_datum,
            tx_hash: c.tx_hash.to_vec(),
            block_number: c.block_number,
            block_hash: c.block_hash.to_vec(),
            tx_index: c.tx_index_in_block,
            utxo_tx_hash: c.utxo_tx_hash.to_vec(),
            utxo_index: c.utxo_index.into(),
            block_timestamp_unix: c.block_timestamp,
        }
    }
}

impl From<&RegistrationEvent> for EpochCandidate {
    fn from(event: &RegistrationEvent) -> Self {
        let full_datum = event.datum.to_bytes().expect("datum should always be inline");

        EpochCandidate {
            full_datum,
            utxo_tx_hash: event.tx_hash.to_vec(),
            utxo_index: event.utxo_index as u32,
            epoch_number: event.epoch,
            block_number: event.block_number,
            slot_number: event.slot_number,
            tx_index: event.tx_index,
            tx_inputs: event
                .tx_inputs
                .iter()
                .map(|i| UtxoId {
                    tx_hash: i.tx_hash.to_vec(),
                    index: i.output_index as u32,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{AssetCreate as AssetCreateInternal, AssetSpend as AssetSpendInternal};
    use acropolis_common::{BlockHash, KeyHash, NetworkId, StakeAddress, StakeCredential, TxHash};

    use super::{AssetCreateProto, AssetSpendProto};

    fn key_hash(byte: u8) -> KeyHash {
        [byte; 28].into()
    }

    fn owner_address(network: NetworkId, byte: u8) -> StakeAddress {
        StakeAddress::new(StakeCredential::AddrKeyHash(key_hash(byte)), network)
    }

    fn asset_create(owner_address: StakeAddress) -> AssetCreateInternal {
        AssetCreateInternal {
            holder_address: owner_address,
            quantity: 10,
            utxo_index: 1,
            tx_hash: TxHash::new([9u8; 32]),
            block_number: 11,
            block_hash: BlockHash::new([10u8; 32]),
            tx_index_in_block: 12,
            block_timestamp: 13,
        }
    }

    fn asset_spend(owner_address: StakeAddress) -> AssetSpendInternal {
        AssetSpendInternal {
            holder_address: owner_address,
            quantity: 10,
            spending_tx_hash: TxHash::new([11u8; 32]),
            block_number: 12,
            block_hash: BlockHash::new([12u8; 32]),
            tx_index_in_block: 13,
            block_timestamp: 14,
            utxo_tx_hash: TxHash::new([13u8; 32]),
            utxo_index: 2,
        }
    }

    #[test]
    fn asset_create_proto_uses_owner_stake_address_bytes() {
        let proto: AssetCreateProto = asset_create(owner_address(NetworkId::Testnet, 2)).into();

        assert_eq!(proto.address.len(), 29);
        assert_eq!(proto.address[0], 0b1110_0000);
    }

    #[test]
    fn asset_spend_proto_uses_owner_stake_address_bytes() {
        let proto: AssetSpendProto = AssetSpendInternal {
            holder_address: StakeAddress::new(
                StakeCredential::ScriptHash(key_hash(4)),
                NetworkId::Mainnet,
            ),
            ..asset_spend(owner_address(NetworkId::Mainnet, 7))
        }
        .into();

        assert_eq!(proto.address.len(), 29);
        assert_eq!(proto.address[0], 0b1111_0001);
    }
}
