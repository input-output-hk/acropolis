// SPDX-License-Identifier: Apache-2.0
// Copyright © 2025, Acropolis team.

//! Streaming snapshot parser with callback interface for bootstrap process.
//!
//! This module provides a callback-based streaming parser for Cardano snapshots
//! that allows processing large snapshots without loading the entire structure
//! into memory. It's designed for the bootstrap process to distribute state
//! via message bus.
//!
//! The parser navigates the NewEpochState structure and invokes callbacks for:
//! - UTXOs (per-entry callback for each UTXO)
//! - Stake pools (bulk callback with all pool data)
//! - Stake accounts (bulk callback with delegations and rewards)
//! - DReps (bulk callback with governance info)
//! - Proposals (bulk callback with active governance actions)
//!
//! Parses CBOR dumps from Cardano Haskell node's GetCBOR ledger-state query.
//! These snapshots represent the internal `NewEpochState` type and are not formally
//! specified - see: https://github.com/IntersectMBO/cardano-ledger/blob/33e90ea03447b44a389985ca2b158568e5f4ad65/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState/Types.hs#L121-L131
//!

use anyhow::{anyhow, Context, Result};
use minicbor::data::Type;
use minicbor::Decoder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

// -----------------------------------------------------------------------------
// Data Structures (based on OpenAPI schema)
// -----------------------------------------------------------------------------

/// UTXO entry with transaction hash, index, address, and value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoEntry {
    /// Transaction hash (hex-encoded)
    pub tx_hash: String,
    /// Output index
    pub output_index: u64,
    /// Bech32-encoded Cardano address
    pub address: String,
    /// Lovelace amount
    pub value: u64,
    /// Optional inline datum (hex-encoded CBOR)
    pub datum: Option<String>,
    /// Optional script reference (hex-encoded CBOR)
    pub script_ref: Option<String>,
}

/// Stake account state (delegations and rewards)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    /// Bech32-encoded stake address
    pub stake_address: String,
    /// Combined Lovelace amount of all UTXOs
    pub utxo_value: u64,
    /// Lovelace amount in reward account
    pub rewards: u64,
    /// Hex-encoded pool ID delegation (if any)
    pub delegated_spo: Option<String>,
    /// DRep delegation (if any)
    pub delegated_drep: Option<DelegatedDRep>,
}

/// DRep delegation target
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegatedDRep {
    /// Bech32-encoded DRep ID
    DRep(String),
    /// Abstain from voting
    Abstain,
    /// No confidence
    NoConfidence,
}

/// Stake pool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    /// Bech32-encoded pool ID
    pub pool_id: String,
    /// Hex-encoded VRF key hash
    pub vrf_key_hash: String,
    /// Pledge amount in Lovelace
    pub pledge: u64,
    /// Fixed cost in Lovelace
    pub cost: u64,
    /// Pool margin (0.0 to 1.0)
    pub margin: f64,
    /// Bech32-encoded reward account
    pub reward_account: String,
    /// List of pool owner stake addresses
    pub pool_owners: Vec<String>,
    /// Pool relay information
    pub relays: Vec<Relay>,
    /// Pool metadata (URL and hash)
    pub pool_metadata: Option<PoolMetadata>,
    /// Optional retirement epoch
    pub retirement_epoch: Option<u64>,
}

/// Pool relay information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Relay {
    SingleHostAddr {
        port: Option<u16>,
        ipv4: Option<String>,
        ipv6: Option<String>,
    },
    SingleHostName {
        port: Option<u16>,
        dns_name: String,
    },
    MultiHostName {
        dns_name: String,
    },
}

/// Pool metadata anchor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMetadata {
    /// IPFS or HTTP(S) URL
    pub url: String,
    /// Hex-encoded hash
    pub hash: String,
}

/// DRep information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DRepInfo {
    /// Bech32-encoded DRep ID
    pub drep_id: String,
    /// Lovelace deposit amount
    pub deposit: u64,
    /// Optional anchor (URL and hash)
    pub anchor: Option<Anchor>,
}

/// Governance proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceProposal {
    /// Lovelace deposit amount
    pub deposit: u64,
    /// Bech32-encoded stake address of proposer
    pub reward_account: String,
    /// Bech32-encoded governance action ID
    pub gov_action_id: String,
    /// Governance action type
    pub gov_action: String,
    /// Anchor information
    pub anchor: Anchor,
}

/// Anchor information (reference URL and data hash)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    /// IPFS or HTTP(S) URL containing anchor data
    pub url: String,
    /// Hex-encoded hash of the anchor data
    pub data_hash: String,
}

/// Pot balances (treasury, reserves, deposits)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotBalances {
    /// Current reserves pot balance in Lovelace
    pub reserves: u64,
    /// Current treasury pot balance in Lovelace
    pub treasury: u64,
    /// Current deposits pot balance in Lovelace
    pub deposits: u64,
}

/// Snapshot metadata extracted before streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Epoch number
    pub epoch: u64,
    /// Pot balances
    pub pot_balances: PotBalances,
    /// Total number of UTXOs (for progress tracking)
    pub utxo_count: Option<u64>,
}

// -----------------------------------------------------------------------------
// Callback Traits
// -----------------------------------------------------------------------------

/// Callback invoked for each UTXO entry (streaming)
pub trait UtxoCallback {
    /// Called once per UTXO entry
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()>;
}

/// Callback invoked with bulk stake pool data
pub trait PoolCallback {
    /// Called once with all pool data
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()>;
}

/// Callback invoked with bulk stake account data
pub trait StakeCallback {
    /// Called once with all account states
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()>;
}

/// Callback invoked with bulk DRep data
pub trait DRepCallback {
    /// Called once with all DRep info
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()>;
}

/// Callback invoked with bulk governance proposal data
pub trait ProposalCallback {
    /// Called once with all proposals
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()>;
}

/// Combined callback handler for all snapshot data
pub trait SnapshotCallbacks:
    UtxoCallback + PoolCallback + StakeCallback + DRepCallback + ProposalCallback
{
    /// Called before streaming begins with metadata
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()>;

    /// Called after all streaming is complete
    fn on_complete(&mut self) -> Result<()>;
}

// -----------------------------------------------------------------------------
// Streaming Parser
// -----------------------------------------------------------------------------

/// Streaming snapshot parser with callback interface
pub struct StreamingSnapshotParser {
    file_path: String,
}

impl StreamingSnapshotParser {
    /// Create a new streaming parser for the given snapshot file
    pub fn new(file_path: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
        }
    }

    /// Parse the snapshot file and invoke callbacks
    ///
    /// This method navigates the NewEpochState structure:
    /// ```text
    /// NewEpochState = [
    ///   0: epoch_no,
    ///   1: blocks_previous_epoch,
    ///   2: blocks_current_epoch,
    ///   3: EpochState = [
    ///        0: AccountState = [treasury, reserves],
    ///        1: LedgerState = [
    ///             0: CertState = [
    ///                  0: VState = [dreps, cc, dormant_epoch],
    ///                  1: PState = [pools, future_pools, retiring, deposits],
    ///                  2: DState = [unified_rewards, fut_gen_deleg, gen_deleg, instant_rewards],
    ///                ],
    ///             1: UTxOState = [
    ///                  0: utxos (map: TxIn -> TxOut),
    ///                  1: deposits,
    ///                  2: fees,
    ///                  3: gov_state,
    ///                  4: donations,
    ///                ],
    ///           ],
    ///        2: PParams,
    ///        3: PParamsPrevious,
    ///      ],
    ///   4: PoolDistr,
    ///   5: StakeDistr,
    /// ]
    /// ```
    pub fn parse<C: SnapshotCallbacks>(&self, callbacks: &mut C) -> Result<()> {
        let mut file = File::open(&self.file_path)
            .context(format!("Failed to open snapshot file: {}", self.file_path))?;

        // Read entire file into memory (minicbor Decoder works with byte slices)
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).context("Failed to read snapshot file")?;

        let mut decoder = Decoder::new(&buffer);

        // Navigate to NewEpochState root array
        let new_epoch_state_len = decoder
            .array()
            .context("Failed to parse NewEpochState root array")?
            .ok_or_else(|| anyhow!("NewEpochState must be a definite-length array"))?;

        if new_epoch_state_len < 4 {
            return Err(anyhow!(
                "NewEpochState array too short: expected at least 4 elements, got {}",
                new_epoch_state_len
            ));
        }

        // Extract epoch number [0]
        let epoch = decoder.u64().context("Failed to parse epoch number")?;

        // Skip blocks_previous_epoch [1] and blocks_current_epoch [2]
        decoder.skip().context("Failed to skip blocks_previous_epoch")?;
        decoder.skip().context("Failed to skip blocks_current_epoch")?;

        // Navigate to EpochState [3]
        let epoch_state_len = decoder
            .array()
            .context("Failed to parse EpochState array")?
            .ok_or_else(|| anyhow!("EpochState must be a definite-length array"))?;

        if epoch_state_len < 3 {
            return Err(anyhow!(
                "EpochState array too short: expected at least 3 elements, got {}",
                epoch_state_len
            ));
        }

        // Extract AccountState [3][0]: [treasury, reserves]
        // Note: In Conway era, AccountState is just [treasury, reserves], not a full map
        let account_state_len = decoder
            .array()
            .context("Failed to parse AccountState array")?
            .ok_or_else(|| anyhow!("AccountState must be a definite-length array"))?;

        if account_state_len < 2 {
            return Err(anyhow!(
                "AccountState array too short: expected at least 2 elements, got {}",
                account_state_len
            ));
        }

        // Parse treasury and reserves (can be negative in CBOR, so decode as i64 first)
        let treasury_i64: i64 = decoder.decode().context("Failed to parse treasury")?;
        let reserves_i64: i64 = decoder.decode().context("Failed to parse reserves")?;
        let treasury = treasury_i64 as u64;
        let reserves = reserves_i64 as u64;

        // Skip any remaining AccountState fields
        for i in 2..account_state_len {
            decoder.skip().context(format!("Failed to skip AccountState[{}]", i))?;
        }

        // Emit metadata callback
        callbacks.on_metadata(SnapshotMetadata {
            epoch,
            pot_balances: PotBalances {
                reserves,
                treasury,
                deposits: 0, // Will be updated from UTxOState
            },
            utxo_count: None, // Unknown until we traverse
        })?;

        // Navigate to LedgerState [3][1]
        let ledger_state_len = decoder
            .array()
            .context("Failed to parse LedgerState array")?
            .ok_or_else(|| anyhow!("LedgerState must be a definite-length array"))?;

        if ledger_state_len < 2 {
            return Err(anyhow!(
                "LedgerState array too short: expected at least 2 elements, got {}",
                ledger_state_len
            ));
        }

        // Parse CertState [3][1][0] to extract DReps and pools
        // CertState (ARRAY) - DReps, pools, accounts
        //       - [0] VotingState - DReps at [3][1][0][0][0]
        //       - [1] PoolState - pools at [3][1][0][1][0]
        //       - [2] DelegationState - accounts at [3][1][0][2][0][0]
        // CertState = [VState, PState, DState]
        let cert_state_len = decoder
            .array()
            .context("Failed to parse CertState array")?
            .ok_or_else(|| anyhow!("CertState must be a definite-length array"))?;

        if cert_state_len < 3 {
            return Err(anyhow!(
                "CertState array too short: expected at least 3 elements, got {}",
                cert_state_len
            ));
        }

        // Parse VState [3][1][0][0] for DReps
        let dreps = Self::parse_vstate(&mut decoder).context("Failed to parse VState for DReps")?;

        // Skip PState [3][1][0][1] for now (pools)
        decoder.skip().context("Failed to skip PState")?;
        let pools = Vec::new(); // TODO: Parse from PState

        // Skip DState [3][1][0][2] for now (accounts/delegations)
        decoder.skip().context("Failed to skip DState")?;

        // Navigate to UTxOState [3][1][1]
        let utxo_state_len = decoder
            .array()
            .context("Failed to parse UTxOState array")?
            .ok_or_else(|| anyhow!("UTxOState must be a definite-length array"))?;

        if utxo_state_len < 1 {
            return Err(anyhow!(
                "UTxOState array too short: expected at least 1 element, got {}",
                utxo_state_len
            ));
        }

        // Stream UTXOs [3][1][1][0] with per-entry callback
        Self::stream_utxos(&mut decoder, callbacks).context("Failed to stream UTXOs")?;

        // Note: We stop here after parsing UTXOs. The remaining fields (deposits, fees, gov_state, etc.)
        // would require more complex parsing. For now, the main goal is UTXO streaming.

        // Emit bulk callbacks (currently empty/stub implementations)
        callbacks.on_pools(pools)?;
        callbacks.on_dreps(dreps)?;
        callbacks.on_accounts(Vec::new())?; // TODO: Parse from DState
        callbacks.on_proposals(Vec::new())?; // TODO: Parse from GovState

        // Emit completion callback
        callbacks.on_complete()?;

        Ok(())
    }

    /// Parse rewards map: stake_credential -> lovelace
    /// TODO: This will be used when rewards are parsed from DState
    #[allow(dead_code)]
    fn parse_rewards_map(decoder: &mut Decoder) -> Result<HashMap<Vec<u8>, u64>> {
        let mut rewards = HashMap::new();

        let map_len = decoder
            .map()
            .context("Failed to parse rewards map")?
            .ok_or_else(|| anyhow!("Rewards map must be definite-length"))?;

        for _ in 0..map_len {
            let credential = decoder.bytes().context("Failed to parse reward credential")?.to_vec();
            let amount = decoder.u64().context("Failed to parse reward amount")?;
            rewards.insert(credential, amount);
        }

        Ok(rewards)
    }

    /// Parse delegations map: stake_credential -> pool_id
    /// TODO: This will be used when delegations are parsed from DState
    #[allow(dead_code)]
    fn parse_delegations_map(decoder: &mut Decoder) -> Result<HashMap<Vec<u8>, Vec<u8>>> {
        let mut delegations = HashMap::new();

        let map_len = decoder
            .map()
            .context("Failed to parse delegations map")?
            .ok_or_else(|| anyhow!("Delegations map must be definite-length"))?;

        for _ in 0..map_len {
            let credential =
                decoder.bytes().context("Failed to parse delegation credential")?.to_vec();
            let pool_id = decoder.bytes().context("Failed to parse pool ID")?.to_vec();
            delegations.insert(credential, pool_id);
        }

        Ok(delegations)
    }

    /// Parse VState to extract DReps
    /// VState = [dreps_map, committee_state, dormant_epoch]
    fn parse_vstate(decoder: &mut Decoder) -> Result<Vec<DRepInfo>> {
        // Parse VState array
        let vstate_len = decoder
            .array()
            .context("Failed to parse VState array")?
            .ok_or_else(|| anyhow!("VState must be a definite-length array"))?;

        if vstate_len < 1 {
            return Err(anyhow!(
                "VState array too short: expected at least 1 element, got {}",
                vstate_len
            ));
        }

        // Parse DReps map [0]: drep_credential -> (deposit, anchor_option)
        let dreps_map_len = decoder.map().context("Failed to parse DReps map")?;

        let mut dreps = Vec::new();

        // Handle both definite and indefinite length maps
        let limit = dreps_map_len.unwrap_or(u64::MAX);

        for _ in 0..limit {
            // Check for break in indefinite map
            if dreps_map_len.is_none() && matches!(decoder.datatype(), Ok(Type::Break)) {
                decoder.skip().ok(); // consume break
                break;
            }
            // Parse DRep credential (key) - can be bytes or array [tag, hash]
            let drep_credential = match decoder.datatype() {
                Ok(Type::Bytes) => {
                    // Simple bytes credential
                    match decoder.bytes() {
                        Ok(bytes) => bytes.to_vec(),
                        Err(e) => {
                            eprintln!("Warning: failed to parse DRep credential bytes: {}", e);
                            decoder.skip().ok(); // skip key
                            decoder.skip().ok(); // skip value
                            continue;
                        }
                    }
                }
                Ok(Type::Array) | Ok(Type::ArrayIndef) => {
                    // Tagged credential: [tag, hash]
                    match decoder.array() {
                        Ok(_) => {
                            // Skip tag
                            decoder.skip().ok();
                            // Get hash bytes
                            match decoder.bytes() {
                                Ok(bytes) => bytes.to_vec(),
                                Err(e) => {
                                    eprintln!(
                                        "Warning: failed to parse DRep credential hash: {}",
                                        e
                                    );
                                    decoder.skip().ok(); // skip value
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Warning: failed to parse DRep credential array: {}", e);
                            decoder.skip().ok(); // skip key
                            decoder.skip().ok(); // skip value
                            continue;
                        }
                    }
                }
                _ => {
                    eprintln!("Warning: unexpected DRep credential type");
                    decoder.skip().ok(); // skip key
                    decoder.skip().ok(); // skip value
                    continue;
                }
            };

            // Parse DRep value: array [deposit, anchor_option]
            match decoder.array() {
                Ok(Some(len)) if len >= 1 => {
                    // Parse deposit amount
                    let deposit = match decoder.u64() {
                        Ok(d) => d,
                        Err(e) => {
                            eprintln!("Warning: failed to parse DRep deposit: {}", e);
                            // Skip remaining array elements
                            for _ in 1..len {
                                decoder.skip().ok();
                            }
                            continue;
                        }
                    };

                    // Parse optional anchor (if present)
                    let anchor = if len >= 2 {
                        Self::parse_anchor_option(decoder).ok().flatten()
                    } else {
                        None
                    };

                    // Skip any remaining fields
                    for _ in 2..len {
                        decoder.skip().ok();
                    }

                    // Create DRep info with hex-encoded credential
                    dreps.push(DRepInfo {
                        drep_id: format!("drep_{}", hex::encode(&drep_credential)),
                        deposit,
                        anchor,
                    });
                }
                Ok(_) => {
                    eprintln!("Warning: DRep value has unexpected format");
                    decoder.skip().ok();
                }
                Err(e) => {
                    eprintln!("Warning: failed to parse DRep value array: {}", e);
                    decoder.skip().ok();
                }
            }
        }

        // Skip committee_state [1] and dormant_epoch [2] if present
        for i in 1..vstate_len {
            decoder.skip().context(format!("Failed to skip VState[{}]", i))?;
        }

        Ok(dreps)
    }

    /// Parse optional anchor: None or Some([url, data_hash])
    fn parse_anchor_option(decoder: &mut Decoder) -> Result<Option<Anchor>> {
        // Check if it's an array (Some) or something else (None)
        match decoder.datatype()? {
            Type::Array | Type::ArrayIndef => {
                let arr_len = decoder.array().context("Failed to parse anchor array")?;

                if arr_len == Some(0) {
                    // Empty array means None
                    return Ok(None);
                }

                // Parse [url, data_hash]
                let url_bytes = decoder.bytes().context("Failed to parse anchor URL")?;
                let url = String::from_utf8_lossy(url_bytes).to_string();

                let data_hash_bytes =
                    decoder.bytes().context("Failed to parse anchor data hash")?;
                let data_hash = hex::encode(data_hash_bytes);

                // Skip any remaining fields
                if let Some(len) = arr_len {
                    for _ in 2..len {
                        decoder.skip().ok();
                    }
                }

                Ok(Some(Anchor { url, data_hash }))
            }
            _ => {
                // Not an array, skip it (represents None)
                decoder.skip().context("Failed to skip non-array anchor")?;
                Ok(None)
            }
        }
    }

    /// Parse DReps from VState (old stub - replaced by parse_vstate)
    #[allow(dead_code)]
    fn parse_dreps(_decoder: &mut Decoder) -> Result<Vec<DRepInfo>> {
        // This function is kept for compatibility but is no longer used
        Ok(Vec::new())
    }

    /// Parse stake pools from PState
    fn parse_pools(_decoder: &mut Decoder) -> Result<Vec<PoolInfo>> {
        // TODO: Implement full pool parsing from PState structure
        // For now, skip PState and return empty vec
        Ok(Vec::new())
    }

    /// Stream UTXOs with per-entry callback
    ///
    /// Note: This uses the existing parse_all_utxos function from snapshot.rs
    /// which has proven robust UTXO parsing logic. We just wrap it with our callback.
    /// Parse a single TxOut from the CBOR decoder
    /// This is copied from snapshot.rs parse_transaction_output logic
    fn parse_transaction_output(dec: &mut Decoder) -> Result<(String, u64)> {
        // TxOut is typically an array [address, value, ...]
        // or a map for Conway with optional fields

        // Try array format first (most common)
        match dec.datatype().context("Failed to read TxOut datatype")? {
            Type::Array | Type::ArrayIndef => {
                let arr_len = dec.array().context("Failed to parse TxOut array")?;
                if arr_len == Some(0) {
                    return Err(anyhow!("empty TxOut array"));
                }

                // Element 0: Address (bytes)
                let address_bytes = dec.bytes().context("Failed to parse address bytes")?;
                let address = hex::encode(address_bytes);

                // Element 1: Value (coin or map)
                let value = match dec.datatype().context("Failed to read value datatype")? {
                    Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                        // Simple ADA-only value
                        dec.u64().context("Failed to parse u64 value")?
                    }
                    Type::Array | Type::ArrayIndef => {
                        // Multi-asset: [coin, assets_map]
                        dec.array().context("Failed to parse value array")?;
                        let coin = dec.u64().context("Failed to parse coin amount")?;
                        // Skip the assets map
                        dec.skip().context("Failed to skip assets map")?;
                        coin
                    }
                    _ => {
                        return Err(anyhow!("unexpected value type"));
                    }
                };

                // Skip remaining fields (datum, script_ref)
                if let Some(len) = arr_len {
                    for _ in 2..len {
                        dec.skip().context("Failed to skip TxOut field")?;
                    }
                }

                Ok((address, value))
            }
            Type::Map | Type::MapIndef => {
                // Map format (Conway with optional fields)
                // Map keys: 0=address, 1=value, 2=datum, 3=script_ref
                let map_len = dec.map().context("Failed to parse TxOut map")?;

                let mut address = String::new();
                let mut value = 0u64;
                let mut found_address = false;
                let mut found_value = false;

                let entries = map_len.unwrap_or(4); // Assume max 4 entries if indefinite
                for _ in 0..entries {
                    // Check for break in indefinite map
                    if map_len.is_none() && matches!(dec.datatype(), Ok(Type::Break)) {
                        dec.skip().ok(); // consume break
                        break;
                    }

                    // Read key
                    let key = match dec.u32() {
                        Ok(k) => k,
                        Err(_) => {
                            // Skip both key and value if key is not u32
                            dec.skip().ok();
                            dec.skip().ok();
                            continue;
                        }
                    };

                    // Read value based on key
                    match key {
                        0 => {
                            // Address
                            if let Ok(addr_bytes) = dec.bytes() {
                                address = hex::encode(addr_bytes);
                                found_address = true;
                            } else {
                                dec.skip().ok();
                            }
                        }
                        1 => {
                            // Value (coin or multi-asset)
                            match dec.datatype() {
                                Ok(Type::U8) | Ok(Type::U16) | Ok(Type::U32) | Ok(Type::U64) => {
                                    if let Ok(coin) = dec.u64() {
                                        value = coin;
                                        found_value = true;
                                    } else {
                                        dec.skip().ok();
                                    }
                                }
                                Ok(Type::Array) | Ok(Type::ArrayIndef) => {
                                    // Multi-asset: [coin, assets_map]
                                    if dec.array().is_ok() {
                                        if let Ok(coin) = dec.u64() {
                                            value = coin;
                                            found_value = true;
                                        }
                                        dec.skip().ok(); // skip assets map
                                    } else {
                                        dec.skip().ok();
                                    }
                                }
                                _ => {
                                    dec.skip().ok();
                                }
                            }
                        }
                        _ => {
                            // datum (2), script_ref (3), or unknown - skip
                            dec.skip().ok();
                        }
                    }
                }

                if found_address && found_value {
                    Ok((address, value))
                } else {
                    Err(anyhow!("map-based TxOut missing required fields"))
                }
            }
            _ => Err(anyhow!("unexpected TxOut type")),
        }
    }

    fn stream_utxos<C: UtxoCallback>(decoder: &mut Decoder, callbacks: &mut C) -> Result<()> {
        // Parse the UTXO map
        let map_len = decoder.map().context("Failed to parse UTxOs map")?;

        let mut count = 0u64;
        let mut errors = 0u64;

        // Determine iteration limit (all entries for definite map, unlimited for indefinite)
        let limit = map_len.unwrap_or(u64::MAX);

        for _ in 0..limit {
            // Check for break in indefinite map
            if map_len.is_none() && matches!(decoder.datatype(), Ok(Type::Break)) {
                break;
            }

            // Progress reporting every million UTXOs
            if count > 0 && count % 1000000 == 0 {
                eprintln!("Parsed {} UTXOs...", count);
            }

            // Parse key: TransactionInput (array [tx_hash, output_index])
            if decoder.array().is_err() {
                break;
            }

            let tx_hash_bytes = match decoder.bytes() {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Warning: failed to parse tx_hash: {}", e);
                    errors += 1;
                    decoder.skip().ok(); // skip remaining TxIn fields and value
                    continue;
                }
            };

            let output_index = match decoder.u64() {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Warning: failed to parse output_index: {}", e);
                    errors += 1;
                    decoder.skip().ok(); // skip value
                    continue;
                }
            };

            let tx_hash = hex::encode(tx_hash_bytes);

            // Parse value: TransactionOutput using proven logic
            match Self::parse_transaction_output(decoder) {
                Ok((address, value)) => {
                    let utxo = UtxoEntry {
                        tx_hash,
                        output_index,
                        address,
                        value,
                        datum: None,      // TODO: Extract from TxOut
                        script_ref: None, // TODO: Extract from TxOut
                    };
                    callbacks.on_utxo(utxo)?;
                    count += 1;
                }
                Err(e) => {
                    eprintln!("Warning: failed to parse UTXO value: {}", e);
                    errors += 1;
                }
            }
        }

        if errors > 0 {
            eprintln!(
                "Warning: {} UTXO parsing errors encountered ({}% success rate)",
                errors,
                (count * 100) / (count + errors)
            );
        }

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Helper: Simple callback handler for testing
// -----------------------------------------------------------------------------

/// Simple callback handler that collects all data in memory (for testing)
#[derive(Debug, Default)]
pub struct CollectingCallbacks {
    pub metadata: Option<SnapshotMetadata>,
    pub utxos: Vec<UtxoEntry>,
    pub pools: Vec<PoolInfo>,
    pub accounts: Vec<AccountState>,
    pub dreps: Vec<DRepInfo>,
    pub proposals: Vec<GovernanceProposal>,
}

impl UtxoCallback for CollectingCallbacks {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()> {
        self.utxos.push(utxo);
        Ok(())
    }
}

impl PoolCallback for CollectingCallbacks {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        self.pools = pools;
        Ok(())
    }
}

impl StakeCallback for CollectingCallbacks {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        self.accounts = accounts;
        Ok(())
    }
}

impl DRepCallback for CollectingCallbacks {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
        self.dreps = dreps;
        Ok(())
    }
}

impl ProposalCallback for CollectingCallbacks {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()> {
        self.proposals = proposals;
        Ok(())
    }
}

impl SnapshotCallbacks for CollectingCallbacks {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        self.metadata = Some(metadata);
        Ok(())
    }

    fn on_complete(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collecting_callbacks() {
        let mut callbacks = CollectingCallbacks::default();

        // Test metadata callback
        callbacks
            .on_metadata(SnapshotMetadata {
                epoch: 507,
                pot_balances: PotBalances {
                    reserves: 1000000,
                    treasury: 2000000,
                    deposits: 500000,
                },
                utxo_count: Some(100),
            })
            .unwrap();

        assert_eq!(callbacks.metadata.as_ref().unwrap().epoch, 507);
        assert_eq!(
            callbacks.metadata.as_ref().unwrap().pot_balances.treasury,
            2000000
        );

        // Test UTXO callback
        callbacks
            .on_utxo(UtxoEntry {
                tx_hash: "abc123".to_string(),
                output_index: 0,
                address: "addr1...".to_string(),
                value: 5000000,
                datum: None,
                script_ref: None,
            })
            .unwrap();

        assert_eq!(callbacks.utxos.len(), 1);
        assert_eq!(callbacks.utxos[0].value, 5000000);
    }
}
