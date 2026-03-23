use anyhow::anyhow;
use uplc_turbo::{arena::Arena, data::PlutusData};

use crate::{
    grpc::midnight_state_proto::{
        bridge_checkpoint, bridge_transfer, bridge_transfers_request,
        AssetCreate as AssetCreateProto, AssetSpend as AssetSpendProto,
        BridgeCheckpoint as BridgeCheckpointProto, BridgeTransfer as BridgeTransferProto,
        Deregistration as DeregistrationProto, EpochCandidate, InvalidBridgeTransfer,
        Registration as RegistrationProto, ReserveBridgeTransfer, UserBridgeTransfer, UtxoId,
    },
    indexes::bridge_state::BridgeCheckpoint,
    types::{
        AssetCreate as AssetCreateInternal, AssetSpend as AssetSpendInternal,
        BridgeAssetUtxo as BridgeAssetUtxoInternal, Deregistration as DeregistrationInternal,
        Registration as RegistrationInternal, RegistrationEvent,
    },
};
use acropolis_common::{TxHash, UTxOIdentifier};

enum BridgeTransferKind {
    UserTransfer { recipient: Vec<u8> },
    ReserveTransfer,
}

pub fn bridge_checkpoint_from_proto(
    checkpoint: Option<bridge_transfers_request::Checkpoint>,
) -> anyhow::Result<BridgeCheckpoint> {
    match checkpoint {
        Some(bridge_transfers_request::Checkpoint::BlockNumber(block_number)) => {
            Ok(BridgeCheckpoint::Block(block_number))
        }
        Some(bridge_transfers_request::Checkpoint::Utxo(utxo)) => {
            Ok(BridgeCheckpoint::Utxo(UTxOIdentifier::new(
                TxHash::try_from(utxo.tx_hash)
                    .map_err(|_| anyhow!("invalid bridge checkpoint tx hash"))?,
                u16::try_from(utxo.index)
                    .map_err(|_| anyhow!("invalid bridge checkpoint output index"))?,
            )))
        }
        None => Err(anyhow!("missing bridge checkpoint")),
    }
}

pub fn bridge_checkpoint_to_proto(checkpoint: BridgeCheckpoint) -> BridgeCheckpointProto {
    let kind = match checkpoint {
        BridgeCheckpoint::Block(block_number) => bridge_checkpoint::Kind::BlockNumber(block_number),
        BridgeCheckpoint::Utxo(utxo) => bridge_checkpoint::Kind::Utxo(UtxoId {
            tx_hash: utxo.tx_hash.to_vec(),
            index: utxo.output_index.into(),
        }),
    };

    BridgeCheckpointProto { kind: Some(kind) }
}

pub fn bridge_transfer_from_utxo(utxo: BridgeAssetUtxoInternal) -> Option<BridgeTransferProto> {
    let token_amount = utxo.tokens_out.checked_sub(utxo.tokens_in)?;
    if token_amount == 0 {
        return None;
    }

    let utxo_id = UtxoId {
        tx_hash: utxo.tx_hash.to_vec(),
        index: utxo.output_index.into(),
    };

    let kind = match utxo.datum {
        None => bridge_transfer::Kind::Invalid(InvalidBridgeTransfer {
            token_amount,
            utxo: Some(utxo_id),
        }),
        Some(datum_bytes) => match decode_bridge_transfer_datum(datum_bytes) {
            Some(BridgeTransferKind::UserTransfer { recipient }) => {
                bridge_transfer::Kind::User(UserBridgeTransfer {
                    token_amount,
                    recipient,
                    utxo: Some(utxo_id),
                })
            }
            Some(BridgeTransferKind::ReserveTransfer) => {
                bridge_transfer::Kind::Reserve(ReserveBridgeTransfer { token_amount })
            }
            None => bridge_transfer::Kind::Invalid(InvalidBridgeTransfer {
                token_amount,
                utxo: Some(utxo_id),
            }),
        },
    };

    Some(BridgeTransferProto { kind: Some(kind) })
}

fn decode_bridge_transfer_datum(datum_bytes: Vec<u8>) -> Option<BridgeTransferKind> {
    let arena = Arena::new();
    let datum = PlutusData::from_cbor(&arena, &datum_bytes).ok()?;
    let fields = match datum {
        PlutusData::List(fields) if fields.len() == 3 => fields,
        _ => return None,
    };
    let appendix = fields[1];
    let version = match fields[2] {
        PlutusData::Integer(version) => *version,
        _ => return None,
    };

    if version != &1.into() {
        return None;
    }

    let (alternative, data) = match appendix {
        PlutusData::Constr { tag, fields } => (*tag, *fields),
        _ => return None,
    };

    match alternative {
        0 if data.len() == 1 => Some(BridgeTransferKind::UserTransfer {
            recipient: match data[0] {
                PlutusData::ByteString(bytes) => bytes.to_vec(),
                _ => return None,
            },
        }),
        1 if data.is_empty() => Some(BridgeTransferKind::ReserveTransfer),
        _ => None,
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
