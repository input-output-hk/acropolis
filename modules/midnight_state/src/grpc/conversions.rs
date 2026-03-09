use crate::{
    grpc::midnight_state_proto::{
        AssetCreate as AssetCreateProto, AssetSpend as AssetSpendProto,
        Deregistration as DeregistrationProto, Registration as RegistrationProto,
    },
    types::{
        AssetCreate as AssetCreateInternal, AssetSpend as AssetSpendInternal,
        Deregistration as DeregistrationInternal, Registration as RegistrationInternal,
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
