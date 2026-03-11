use crate::{
    grpc::midnight_state_proto::{
        AssetCreate as AssetCreateProto, AssetSpend as AssetSpendProto,
        Deregistration as DeregistrationProto, EpochCandidate, Registration as RegistrationProto,
        UtxoId,
    },
    types::{
        AssetCreate as AssetCreateInternal, AssetSpend as AssetSpendInternal,
        Deregistration as DeregistrationInternal, Registration as RegistrationInternal,
        RegistrationEvent,
    },
};
use acropolis_common::{Address, ShelleyAddressDelegationPart, StakeAddress, StakeCredential};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CNightOwnerAddressError {
    UnsupportedAddressKind(&'static str),
    MissingDelegation,
    PointerDelegation,
}

impl Display for CNightOwnerAddressError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedAddressKind(kind) => {
                write!(f, "{kind} addresses are not supported for cNIGHT ownership")
            }
            Self::MissingDelegation => {
                write!(
                    f,
                    "holder address has no delegation part to derive owner stake address"
                )
            }
            Self::PointerDelegation => {
                write!(
                    f,
                    "holder address uses pointer delegation and cannot derive owner stake address"
                )
            }
        }
    }
}

fn encode_cnight_owner_address(address: &Address) -> Result<Vec<u8>, CNightOwnerAddressError> {
    let stake_address = match address {
        Address::Shelley(shelley) => match &shelley.delegation {
            ShelleyAddressDelegationPart::StakeKeyHash(hash) => {
                StakeAddress::new(StakeCredential::AddrKeyHash(*hash), shelley.network.clone())
            }
            ShelleyAddressDelegationPart::ScriptHash(hash) => {
                StakeAddress::new(StakeCredential::ScriptHash(*hash), shelley.network.clone())
            }
            ShelleyAddressDelegationPart::Pointer(_) => {
                return Err(CNightOwnerAddressError::PointerDelegation);
            }
            ShelleyAddressDelegationPart::None => {
                return Err(CNightOwnerAddressError::MissingDelegation);
            }
        },
        Address::Stake(stake) => stake.clone(),
        Address::Byron(_) => {
            return Err(CNightOwnerAddressError::UnsupportedAddressKind("byron"));
        }
        Address::None => {
            return Err(CNightOwnerAddressError::UnsupportedAddressKind("none"));
        }
    };

    Ok(stake_address.to_bytes_key())
}

pub(crate) fn try_asset_create_proto(
    c: &AssetCreateInternal,
) -> Result<AssetCreateProto, CNightOwnerAddressError> {
    Ok(AssetCreateProto {
        address: encode_cnight_owner_address(&c.holder_address)?,
        quantity: c.quantity,
        tx_hash: c.tx_hash.to_vec(),
        output_index: c.utxo_index.into(),
        block_number: c.block_number,
        block_hash: c.block_hash.to_vec(),
        tx_index: c.tx_index_in_block,
        block_timestamp_unix: c.block_timestamp,
    })
}

pub(crate) fn try_asset_spend_proto(
    c: &AssetSpendInternal,
) -> Result<AssetSpendProto, CNightOwnerAddressError> {
    Ok(AssetSpendProto {
        address: encode_cnight_owner_address(&c.holder_address)?,
        quantity: c.quantity,
        spending_tx_hash: c.spending_tx_hash.to_vec(),
        block_number: c.block_number,
        block_hash: c.block_hash.to_vec(),
        tx_index: c.tx_index_in_block,
        utxo_tx_hash: c.utxo_tx_hash.to_vec(),
        utxo_index: c.utxo_index.into(),
        block_timestamp_unix: c.block_timestamp,
    })
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
    use super::{
        encode_cnight_owner_address, try_asset_create_proto, try_asset_spend_proto,
        CNightOwnerAddressError,
    };
    use crate::types::{AssetCreate as AssetCreateInternal, AssetSpend as AssetSpendInternal};
    use acropolis_common::{
        Address, BlockHash, ByronAddress, KeyHash, NetworkId, ShelleyAddress,
        ShelleyAddressDelegationPart, ShelleyAddressPaymentPart, StakeAddress, StakeCredential,
        TxHash,
    };

    fn key_hash(byte: u8) -> KeyHash {
        [byte; 28].into()
    }

    fn base_key_address(network: NetworkId) -> Address {
        Address::Shelley(ShelleyAddress {
            network,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(key_hash(1)),
            delegation: ShelleyAddressDelegationPart::StakeKeyHash(key_hash(2)),
        })
    }

    fn base_script_address(network: NetworkId) -> Address {
        Address::Shelley(ShelleyAddress {
            network,
            payment: ShelleyAddressPaymentPart::ScriptHash(key_hash(3)),
            delegation: ShelleyAddressDelegationPart::ScriptHash(key_hash(4)),
        })
    }

    fn enterprise_address() -> Address {
        Address::Shelley(ShelleyAddress {
            network: NetworkId::Testnet,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(key_hash(5)),
            delegation: ShelleyAddressDelegationPart::None,
        })
    }

    fn pointer_address() -> Address {
        Address::Shelley(ShelleyAddress {
            network: NetworkId::Testnet,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(key_hash(6)),
            delegation: ShelleyAddressDelegationPart::Pointer(Default::default()),
        })
    }

    fn stake_address(network: NetworkId) -> Address {
        Address::Stake(StakeAddress::new(
            StakeCredential::AddrKeyHash(key_hash(7)),
            network,
        ))
    }

    fn asset_create(holder_address: Address) -> AssetCreateInternal {
        AssetCreateInternal {
            holder_address,
            quantity: 10,
            tx_hash: TxHash::new([9u8; 32]),
            utxo_index: 1,
            block_number: 11,
            block_hash: BlockHash::new([10u8; 32]),
            tx_index_in_block: 12,
            block_timestamp: 13,
        }
    }

    fn asset_spend(holder_address: Address) -> AssetSpendInternal {
        AssetSpendInternal {
            holder_address,
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
    fn encodes_owner_address_from_stake_key_delegation() {
        let address = base_key_address(NetworkId::Testnet);
        let expected = StakeAddress::new(
            StakeCredential::AddrKeyHash(key_hash(2)),
            NetworkId::Testnet,
        )
        .to_bytes_key();

        let actual = encode_cnight_owner_address(&address).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 29);
    }

    #[test]
    fn encodes_owner_address_from_script_delegation() {
        let address = base_script_address(NetworkId::Mainnet);
        let expected =
            StakeAddress::new(StakeCredential::ScriptHash(key_hash(4)), NetworkId::Mainnet)
                .to_bytes_key();

        let actual = encode_cnight_owner_address(&address).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 29);
    }

    #[test]
    fn passes_stake_address_through_unchanged() {
        let address = stake_address(NetworkId::Testnet);
        let expected = match &address {
            Address::Stake(stake) => stake.to_bytes_key(),
            _ => unreachable!(),
        };

        let actual = encode_cnight_owner_address(&address).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn rejects_unsupported_owner_address_shapes() {
        assert_eq!(
            encode_cnight_owner_address(&enterprise_address()),
            Err(CNightOwnerAddressError::MissingDelegation)
        );
        assert_eq!(
            encode_cnight_owner_address(&pointer_address()),
            Err(CNightOwnerAddressError::PointerDelegation)
        );
        assert_eq!(
            encode_cnight_owner_address(&Address::Byron(ByronAddress {
                payload: vec![1, 2, 3]
            })),
            Err(CNightOwnerAddressError::UnsupportedAddressKind("byron"))
        );
        assert_eq!(
            encode_cnight_owner_address(&Address::None),
            Err(CNightOwnerAddressError::UnsupportedAddressKind("none"))
        );
    }

    #[test]
    fn asset_create_proto_uses_owner_stake_address_bytes() {
        let proto = try_asset_create_proto(&asset_create(base_key_address(NetworkId::Testnet)))
            .expect("asset create should encode owner");

        assert_eq!(proto.address.len(), 29);
        assert_eq!(proto.address[0], 0b1110_0000);
    }

    #[test]
    fn asset_spend_proto_uses_owner_stake_address_bytes() {
        let proto = try_asset_spend_proto(&asset_spend(base_script_address(NetworkId::Mainnet)))
            .expect("asset spend should encode owner");

        assert_eq!(proto.address.len(), 29);
        assert_eq!(proto.address[0], 0b1111_0001);
    }

    #[test]
    fn asset_proto_conversion_rejects_unsupported_addresses() {
        assert_eq!(
            try_asset_create_proto(&asset_create(enterprise_address())),
            Err(CNightOwnerAddressError::MissingDelegation)
        );
        assert_eq!(
            try_asset_spend_proto(&asset_spend(pointer_address())),
            Err(CNightOwnerAddressError::PointerDelegation)
        );
    }
}
