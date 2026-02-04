use crate::{
    Address, Datum, ScriptHash, ShelleyAddressDelegationPart, StakeCredential, TxHash, Value,
};

// Full UTXO identifier as used in the outside world, with TX hash and output index
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct UTxOIdentifier {
    #[n(0)]
    pub tx_hash: TxHash,

    #[n(1)]
    pub output_index: u16,
}

impl UTxOIdentifier {
    pub fn new(tx_hash: TxHash, output_index: u16) -> Self {
        UTxOIdentifier {
            tx_hash,
            output_index,
        }
    }

    /// Get the transaction hash as a hex string
    pub fn tx_hash_hex(&self) -> String {
        self.tx_hash.to_string()
    }

    pub fn to_bytes(&self) -> [u8; 34] {
        let mut buf = [0u8; 34];
        buf[..32].copy_from_slice(self.tx_hash.as_inner());
        buf[32..34].copy_from_slice(&self.output_index.to_be_bytes());
        buf
    }
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        if bytes.len() != 34 {
            return Err(anyhow::anyhow!(
                "Invalid UTxOIdentifier bytes length: expected 34, got {}",
                bytes.len()
            ));
        }
        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(&bytes[..32]);
        let tx_hash = TxHash::from(hash_bytes);
        let output_index = u16::from_be_bytes([bytes[32], bytes[33]]);
        Ok(Self {
            tx_hash,
            output_index,
        })
    }
}

impl std::fmt::Display for UTxOIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.tx_hash, self.output_index)
    }
}

/// Value stored in UTXO
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct UTXOValue {
    /// Address in binary
    pub address: Address,

    /// Value in Lovelace
    pub value: Value,

    /// Datum
    pub datum: Option<Datum>,

    /// Reference script hash
    pub reference_script_hash: Option<ScriptHash>,
}

impl UTXOValue {
    /// Get the coin (lovelace) value
    pub fn coin(&self) -> u64 {
        self.value.lovelace
    }

    /// Get the address as raw bytes
    pub fn address_bytes(&self) -> Vec<u8> {
        match &self.address {
            Address::Shelley(shelley) => shelley.to_bytes_key(),
            Address::Byron(byron) => byron.payload.clone(),
            Address::Stake(stake) => stake.to_binary(),
            Address::None => Vec::new(),
        }
    }

    /// Extract the stake credential from the address, if present.
    ///
    /// Returns `Some(StakeCredential)` for Shelley addresses that have
    /// a stake key or script hash delegation. Returns `None` for:
    /// - Byron addresses
    /// - Enterprise addresses (no delegation)
    /// - Pointer addresses (delegation is a pointer, not a credential)
    /// - Stake/reward addresses
    pub fn extract_stake_credential(&self) -> Option<StakeCredential> {
        match &self.address {
            Address::Shelley(shelley) => match &shelley.delegation {
                ShelleyAddressDelegationPart::StakeKeyHash(hash) => {
                    Some(StakeCredential::AddrKeyHash(*hash))
                }
                ShelleyAddressDelegationPart::ScriptHash(hash) => {
                    Some(StakeCredential::ScriptHash(*hash))
                }
                _ => None,
            },
            _ => None,
        }
    }
}
