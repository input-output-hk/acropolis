use crate::{
    grpc::midnight_state_proto::{
        AssetCreate as AssetCreateProto, AssetSpend as AssetSpendProto, Block as BlockProto,
        Deregistration as DeregistrationProto, EpochCandidate, Registration as RegistrationProto,
        UtxoId,
    },
    types::{
        AssetCreate as AssetCreateInternal, AssetSpend as AssetSpendInternal,
        Deregistration as DeregistrationInternal, Registration as RegistrationInternal,
        RegistrationEvent,
    },
};
use acropolis_common::queries::blocks::BlockInfo as BlockInfoInternal;
use tonic::Status;

impl TryFrom<BlockInfoInternal> for BlockProto {
    type Error = Status;

    fn try_from(block: BlockInfoInternal) -> Result<Self, Self::Error> {
        Ok(BlockProto {
            block_number: u32::try_from(block.number)
                .map_err(|_| Status::internal("block number overflow"))?,
            block_hash: block.hash.to_vec(),
            epoch_number: u32::try_from(block.epoch)
                .map_err(|_| Status::internal("epoch overflow"))?,
            slot_number: block.slot,
            block_timestamp_unix: block.timestamp,
        })
    }
}

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
            block_timestamp_unix_millis: c.block_timestamp.saturating_mul(1000),
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
            block_timestamp_unix_millis: c.block_timestamp.saturating_mul(1000),
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
            block_timestamp_unix_millis: c.block_timestamp.saturating_mul(1000),
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
            block_timestamp_unix_millis: c.block_timestamp.saturating_mul(1000),
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
    use acropolis_common::{
        queries::blocks::BlockInfo as BlockInfoInternal, BlockHash, KeyHash, NetworkId,
        StakeAddress, StakeCredential, TxHash,
    };

    use super::{AssetCreateProto, AssetSpendProto, BlockProto};

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

    fn block_info() -> BlockInfoInternal {
        BlockInfoInternal {
            timestamp: 13,
            number: 11,
            hash: BlockHash::new([10u8; 32]),
            slot: 12,
            epoch: 14,
            epoch_slot: 15,
            issuer: None,
            size: 16,
            tx_count: 17,
            output: None,
            fees: None,
            block_vrf: None,
            op_cert: None,
            op_cert_counter: None,
            previous_block: None,
            next_block: None,
            confirmations: 18,
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

    #[test]
    fn block_proto_preserves_block_fields() {
        let proto = BlockProto::try_from(block_info()).expect("block info should convert");

        assert_eq!(proto.block_number, 11);
        assert_eq!(proto.block_hash, vec![10u8; 32]);
        assert_eq!(proto.epoch_number, 14);
        assert_eq!(proto.slot_number, 12);
        assert_eq!(proto.block_timestamp_unix, 13);
    }
}
