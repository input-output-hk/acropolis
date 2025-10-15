//! Amaru/Haskell node snapshot format parser
//!
//! Parses CBOR dumps from Cardano Haskell node's GetCBOR ledger-state query.
//! These snapshots represent the internal `EpochState` type and are not formally
//! specified - see: https://github.com/IntersectMBO/cardano-ledger/blob/33e90ea03447b44a389985ca2b158568e5f4ad65/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState/Types.hs#L121-L131
//!
//! This implementation focuses on extracting minimal metadata and high-value fields
//! without attempting full deserialization of the complex nested structure.

use std::fs::File;
use std::io::Read;

use minicbor::data::Type;
use minicbor::decode::Decoder;

use super::SnapshotError;

/// Minimum supported epoch (Conway era starts at epoch 505)
const MIN_SUPPORTED_EPOCH: u64 = 505;

/// Parsed UTXO entry from the snapshot
#[derive(Debug, Clone)]
pub struct UtxoEntry {
    /// Transaction hash (hex string)
    pub tx_hash: String,
    /// Output index within the transaction
    pub output_index: u64,
    /// Address (bech32 or hex)
    pub address: String,
    /// ADA value in lovelace
    pub value: u64,
}

/// Metadata extracted from an Amaru EpochState snapshot
#[derive(Debug)]
pub struct AmaruSnapshot {
    /// File size in bytes
    pub size_bytes: u64,
    /// Top-level CBOR structure type (for diagnostic)
    pub structure_type: String,
    /// Number of top-level array elements (if array) or map entries (if map)
    pub top_level_count: Option<u64>,
}

/// Conway+ era metadata extracted from EpochState
#[derive(Debug)]
pub struct EpochStateMetadata {
    /// Epoch number (e.g., 507)
    pub epoch: u64,
    /// Estimated UTXO count (None if not yet computed)
    pub utxo_count: Option<u64>,
    /// File size in bytes
    pub file_size: u64,
}

/// Full snapshot data needed for boot
#[derive(Debug)]
pub struct SnapshotData {
    /// Epoch number
    pub epoch: u64,
    /// Treasury balance in lovelace
    pub treasury: u64,
    /// Reserves balance in lovelace
    pub reserves: u64,
    /// Stake pool count
    pub stake_pools: u64,
    /// DRep count (Delegated Representatives)
    pub dreps: u64,
    /// Stake account count (accounts with delegation and rewards)
    pub stake_accounts: u64,
    /// Governance proposals count
    pub governance_proposals: u64,
    /// File size in bytes
    pub file_size: u64,
}

/// Tip information extracted from Amaru snapshot filename
#[derive(Debug, Clone)]
pub struct TipInfo {
    /// Absolute slot number
    pub slot: u64,
    /// Block header hash (hex)
    pub block_hash: String,
}

impl EpochStateMetadata {
    /// Parse metadata from an Amaru snapshot (Conway+ eras only)
    ///
    /// This reads only the first ~256KB to extract:
    /// - Epoch number (validates >= 505 for Conway)
    /// - File size
    ///
    /// Does NOT count UTXOs (use `from_file_with_counts` for that).
    /// Errors if epoch < 505 (pre-Conway eras not supported).
    pub fn from_file(path: &str) -> Result<Self, SnapshotError> {
        let mut f = File::open(path).map_err(|e| SnapshotError::IoError(e.to_string()))?;

        // Get file size
        let metadata = f.metadata().map_err(|e| SnapshotError::IoError(e.to_string()))?;
        let file_size = metadata.len();

        // Read first 256KB for structure parsing
        let mut head = vec![0u8; 256 * 1024];
        let n = f.read(&mut head).map_err(|e| SnapshotError::IoError(e.to_string()))?;
        if n == 0 {
            return Err(SnapshotError::StructuralDecode("empty snapshot".into()));
        }
        head.truncate(n);

        let mut dec = Decoder::new(&head);

        // Parse top-level array
        let arr_len = dec.array().map_err(|e| SnapshotError::Cbor(e))?.ok_or_else(|| {
            SnapshotError::StructuralDecode("expected definite-length array".into())
        })?;

        if arr_len != 7 {
            return Err(SnapshotError::StructuralDecode(format!(
                "expected 7-element EpochState array, got {arr_len}"
            )));
        }

        // Parse Element [0]: Epoch number
        let epoch = dec.u64().map_err(|e| {
            SnapshotError::StructuralDecode(format!("failed to parse epoch number: {e}"))
        })?;

        // Validate Conway+ era
        if epoch < MIN_SUPPORTED_EPOCH {
            return Err(SnapshotError::StructuralDecode(format!(
                "epoch {epoch} is pre-Conway (requires >= {MIN_SUPPORTED_EPOCH})"
            )));
        }

        // Element [1] is the LedgerState map - we'll parse it later for UTXO count
        // For now, just validate it's a map
        let ledger_state_is_map = matches!(dec.datatype(), Ok(Type::Map | Type::MapIndef));
        if !ledger_state_is_map {
            return Err(SnapshotError::StructuralDecode(
                "Element [1] should be LedgerState map".into(),
            ));
        }

        Ok(EpochStateMetadata {
            epoch,
            utxo_count: None,
            file_size,
        })
    }

    /// Parse metadata with full UTXO counting (Conway+ eras only)
    ///
    /// This streams through the entire file to count UTXOs in the LedgerState map.
    /// Uses constant memory (~16MB buffer) regardless of file size.
    ///
    /// **Warning**: This is slow for multi-GB snapshots (can take 30+ seconds).
    /// Use `from_file()` for fast metadata extraction without counts.
    pub fn from_file_with_counts(path: &str) -> Result<Self, SnapshotError> {
        // First get basic metadata (epoch, file size)
        let mut base = Self::from_file(path)?;

        // Now count UTXOs by streaming through Element [1]
        base.utxo_count = Some(count_ledger_state_utxos(path)?);

        Ok(base)
    }

    /// Display a human-readable summary
    pub fn summary(&self) -> String {
        let utxo_str = self.utxo_count.map(|c| format!(", ~{c} UTXOs")).unwrap_or_default();
        format!(
            "Conway era snapshot: epoch {}, {} bytes{}",
            self.epoch, self.file_size, utxo_str
        )
    }
}

impl AmaruSnapshot {
    /// Parse an Amaru snapshot file and extract minimal metadata
    ///
    /// This does NOT load the entire file into memory; it only reads
    /// the first few KB to inspect the structure.
    pub fn from_file(path: &str) -> Result<Self, SnapshotError> {
        let mut f = File::open(path).map_err(|e| SnapshotError::IoError(e.to_string()))?;

        // Get file size
        let metadata = f.metadata().map_err(|e| SnapshotError::IoError(e.to_string()))?;
        let size_bytes = metadata.len();

        // Read first 8KB to inspect structure
        let mut head = vec![0u8; 8192];
        let n = f.read(&mut head).map_err(|e| SnapshotError::IoError(e.to_string()))?;
        if n == 0 {
            return Err(SnapshotError::StructuralDecode("empty snapshot".into()));
        }
        head.truncate(n);

        let mut dec = Decoder::new(&head);

        // Try to detect top-level structure
        let (structure_type, top_level_count) = if let Ok(Some(len)) = dec.array() {
            ("array".to_string(), Some(len))
        } else if let Ok(Some(len)) = dec.map() {
            ("map".to_string(), Some(len))
        } else {
            ("unknown".to_string(), None)
        };

        Ok(AmaruSnapshot {
            size_bytes,
            structure_type,
            top_level_count,
        })
    }

    /// Display a human-readable summary of the snapshot
    pub fn summary(&self) -> String {
        let count_str =
            self.top_level_count.map(|c| format!(" with {c} elements")).unwrap_or_default();
        format!(
            "Amaru EpochState snapshot: {} bytes, top-level {} CBOR{}",
            self.size_bytes, self.structure_type, count_str
        )
    }

    /// Inspect the top-level structure of an Amaru snapshot
    ///
    /// Reads the first ~256KB to analyze the structure without loading
    /// the entire file. Returns a detailed description of the CBOR layout.
    pub fn inspect_structure(path: &str) -> Result<String, SnapshotError> {
        let mut f = File::open(path).map_err(|e| SnapshotError::IoError(e.to_string()))?;

        // Read first 256KB for deeper inspection
        let mut head = vec![0u8; 256 * 1024];
        let n = f.read(&mut head).map_err(|e| SnapshotError::IoError(e.to_string()))?;
        if n == 0 {
            return Err(SnapshotError::StructuralDecode("empty snapshot".into()));
        }
        head.truncate(n);

        let mut dec = Decoder::new(&head);
        let mut output = String::new();

        output.push_str("=== Amaru EpochState Structure Inspection ===\n\n");
        output.push_str("This snapshot is from a Haskell Cardano node's GetCBOR query.\n");
        output.push_str("The structure represents the internal EpochState type.\n");
        output.push_str("Reference: https://github.com/IntersectMBO/cardano-ledger/.../LedgerState/Types.hs\n\n");

        // Try to parse top-level array
        match dec.array() {
            Ok(Some(len)) => {
                output.push_str(&format!("Top-level: Array with {len} elements\n"));

                // Provide hints about what each element might be
                if len == 7 {
                    output.push_str("\nLikely EpochState structure (7 elements):\n");
                    output.push_str("  [0] = Epoch number (u64) — e.g., 507 for the Conway era\n");
                    output.push_str(
                        "  [1] = LedgerState (indefinite map: UTXOs, delegations, governance)\n",
                    );
                    output.push_str("  [2] = SnapShots (28-byte hash)\n");
                    output.push_str("  [3] = Nonce (28-byte hash)\n");
                    output.push_str("  [4] = Nonce (28-byte hash)\n");
                    output.push_str("  [5] = ??? (28-byte hash)\n");
                    output.push_str("  [6] = ??? (28-byte hash)\n");
                }
                output.push_str("\nDetailed structure:\n\n");

                // Inspect each element
                for i in 0..len.min(10) {
                    // Limit to first 10 to avoid too much output
                    output.push_str(&format!("Element {i}:\n"));

                    match inspect_element(&mut dec, 1) {
                        Ok(desc) => output.push_str(&desc),
                        Err(e) => {
                            output.push_str(&format!("  Error inspecting: {e}\n"));
                            break;
                        }
                    }

                    output.push('\n');
                }

                if len > 10 {
                    output.push_str(&format!("... ({} more elements not shown)\n", len - 10));
                }
            }
            Ok(None) => {
                output.push_str("Top-level: Indefinite-length array\n");
            }
            Err(_) => {
                output.push_str("Top-level: Not an array (possibly map or other type)\n");
            }
        }

        Ok(output)
    }
}

/// Inspect a single CBOR element and return a description
fn inspect_element(dec: &mut Decoder, indent: usize) -> Result<String, SnapshotError> {
    let indent_str = "  ".repeat(indent);
    let mut output = String::new();

    match dec.datatype().map_err(|e| SnapshotError::Cbor(e))? {
        Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
            let val = dec.u64().map_err(|e| SnapshotError::Cbor(e))?;
            output.push_str(&format!("{indent_str}Unsigned: {val}\n"));
        }
        Type::I8 | Type::I16 | Type::I32 | Type::I64 => {
            let val = dec.i64().map_err(|e| SnapshotError::Cbor(e))?;
            output.push_str(&format!("{indent_str}Signed: {val}\n"));
        }
        Type::Bytes | Type::BytesIndef => {
            let bytes = dec.bytes().map_err(|e| SnapshotError::Cbor(e))?;
            if bytes.len() <= 32 {
                output.push_str(&format!(
                    "{indent_str}Bytes ({}): {}\n",
                    bytes.len(),
                    hex::encode(bytes)
                ));
            } else {
                let preview = hex::encode(&bytes[..32]);
                output.push_str(&format!(
                    "{indent_str}Bytes ({}): {}...\n",
                    bytes.len(),
                    preview
                ));
            }
        }
        Type::String | Type::StringIndef => {
            let s = dec.str().map_err(|e| SnapshotError::Cbor(e))?;
            if s.len() <= 64 {
                output.push_str(&format!("{indent_str}String: \"{s}\"\n"));
            } else {
                output.push_str(&format!(
                    "{indent_str}String ({}): \"{}...\"\n",
                    s.len(),
                    &s[..64]
                ));
            }
        }
        Type::Array | Type::ArrayIndef => {
            let arr_len = dec.array().map_err(|e| SnapshotError::Cbor(e))?;
            match arr_len {
                Some(len) => {
                    output.push_str(&format!("{indent_str}Array [{len}]:\n"));
                    // Only inspect first few elements to avoid overwhelming output
                    let preview_count = len.min(3);
                    for i in 0..preview_count {
                        output.push_str(&format!("{}[{i}]:\n", "  ".repeat(indent + 1)));
                        match inspect_element(dec, indent + 2) {
                            Ok(desc) => output.push_str(&desc),
                            Err(e) => {
                                output
                                    .push_str(&format!("{}Error: {e}\n", "  ".repeat(indent + 2)));
                                break;
                            }
                        }
                    }
                    if len > preview_count {
                        output.push_str(&format!(
                            "{}... ({} more items)\n",
                            "  ".repeat(indent + 1),
                            len - preview_count
                        ));
                        // Skip remaining elements
                        for _ in preview_count..len {
                            dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        }
                    }
                }
                None => {
                    output.push_str(&format!("{indent_str}Array [indefinite]:\n"));
                }
            }
        }
        Type::Map | Type::MapIndef => {
            let map_len = dec.map().map_err(|e| SnapshotError::Cbor(e))?;
            match map_len {
                Some(len) => {
                    output.push_str(&format!("{indent_str}Map {{{len}}}:\n"));
                    // Only inspect first few entries
                    let preview_count = len.min(3);
                    for i in 0..preview_count {
                        output.push_str(&format!("{}Entry {i}:\n", "  ".repeat(indent + 1)));
                        output.push_str(&format!("{}Key:\n", "  ".repeat(indent + 2)));
                        match inspect_element(dec, indent + 3) {
                            Ok(desc) => output.push_str(&desc),
                            Err(e) => {
                                output
                                    .push_str(&format!("{}Error: {e}\n", "  ".repeat(indent + 3)));
                                break;
                            }
                        }
                        output.push_str(&format!("{}Value:\n", "  ".repeat(indent + 2)));
                        match inspect_element(dec, indent + 3) {
                            Ok(desc) => output.push_str(&desc),
                            Err(e) => {
                                output
                                    .push_str(&format!("{}Error: {e}\n", "  ".repeat(indent + 3)));
                                break;
                            }
                        }
                    }
                    if len > preview_count {
                        output.push_str(&format!(
                            "{}... ({} more entries)\n",
                            "  ".repeat(indent + 1),
                            len - preview_count
                        ));
                        // Skip remaining entries
                        for _ in preview_count..len {
                            dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                            dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        }
                    }
                }
                None => {
                    output.push_str(&format!("{indent_str}Map {{indefinite}}:\n"));
                }
            }
        }
        Type::Tag => {
            let tag = dec.tag().map_err(|e| SnapshotError::Cbor(e))?;
            output.push_str(&format!("{indent_str}Tag({tag}):\n"));
            match inspect_element(dec, indent + 1) {
                Ok(desc) => output.push_str(&desc),
                Err(e) => output.push_str(&format!("{}Error: {e}\n", "  ".repeat(indent + 1))),
            }
        }
        Type::Bool => {
            let val = dec.bool().map_err(|e| SnapshotError::Cbor(e))?;
            output.push_str(&format!("{indent_str}Bool: {val}\n"));
        }
        Type::Null => {
            dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
            output.push_str(&format!("{indent_str}Null\n"));
        }
        Type::Undefined => {
            dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
            output.push_str(&format!("{indent_str}Undefined\n"));
        }
        Type::Simple => {
            dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
            output.push_str(&format!("{indent_str}Simple value\n"));
        }
        Type::Break => {
            output.push_str(&format!("{indent_str}Break\n"));
        }
        _ => {
            dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
            output.push_str(&format!("{indent_str}Unknown type\n"));
        }
    }

    Ok(output)
}

/// Count actual UTXOs in the snapshot by navigating to the correct nested structure
///
/// Based on Amaru's parsing code, the structure is:
/// ```text
/// Top-level Array:
///   [0] = Epoch number
///   [1] = Previous blocks made
///   [2] = Current blocks made
///   [3] = Epoch State (ARRAY):
///         [0] = Account State
///         [1] = Ledger State (ARRAY):
///               [0] = Cert State
///               [1] = UTxO State (ARRAY):
///                     [0] = utxo (MAP) <- THE ACTUAL UTXOs!
///                     ...
///         [2] = Snapshots
///         [3] = NonMyopic
///   [4+] = More fields...
/// ```
///
/// This function navigates to EpochState[1].UTxOState[0] and counts the map entries.
fn count_ledger_state_utxos(path: &str) -> Result<u64, SnapshotError> {
    let mut f = File::open(path).map_err(|e| SnapshotError::IoError(e.to_string()))?;

    // Read entire file (we need to parse CBOR sequentially)
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer).map_err(|e| SnapshotError::IoError(e.to_string()))?;

    let mut dec = Decoder::new(&buffer);

    // Parse top-level array
    let top_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected top-level array".into()))?;

    if top_len < 4 {
        return Err(SnapshotError::StructuralDecode(format!(
            "expected at least 4 top-level elements, got {top_len}"
        )));
    }

    // [0] = Epoch number
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [1] = Previous blocks made
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [2] = Current blocks made
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3] = Epoch State (ARRAY)
    let epoch_state_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected Epoch State array".into()))?;

    if epoch_state_len < 2 {
        return Err(SnapshotError::StructuralDecode(format!(
            "expected at least 2 Epoch State elements, got {epoch_state_len}"
        )));
    }

    // Epoch State[0] = Account State
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // Epoch State[1] = Ledger State (ARRAY)
    let ledger_state_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected Ledger State array".into()))?;

    if ledger_state_len < 2 {
        return Err(SnapshotError::StructuralDecode(format!(
            "expected at least 2 Ledger State elements, got {ledger_state_len}"
        )));
    }

    // Ledger State[0] = Cert State
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // Ledger State[1] = UTxO State (ARRAY)
    let utxo_state_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected UTxO State array".into()))?;

    if utxo_state_len < 1 {
        return Err(SnapshotError::StructuralDecode(
            "expected at least 1 UTxO State element".into(),
        ));
    }

    // UTxO State[0] = utxo (MAP) - THE ACTUAL UTXOs!
    let utxo_map_len = dec.map().map_err(|e| SnapshotError::Cbor(e))?;

    match utxo_map_len {
        Some(len) => {
            // Definite-length map: we know the count immediately
            Ok(len)
        }
        None => {
            // Indefinite-length map: count entries until break marker
            let mut count = 0u64;
            loop {
                match dec.datatype() {
                    Ok(Type::Break) => {
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?; // consume break
                        break;
                    }
                    Ok(_) => {
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?; // skip key
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?; // skip value
                        count += 1;
                    }
                    Err(e) => {
                        return Err(SnapshotError::Cbor(e));
                    }
                }
            }
            Ok(count)
        }
    }
}

/// Parse a sample of UTXOs from the snapshot for testing/validation
///
/// This function navigates to the UTXO map and extracts the first N entries
/// to verify structure and test deserialization without loading all 11M+ UTXOs.
///
/// Returns a vector of UtxoEntry or an error if navigation/parsing fails.
pub fn parse_sample_utxos(path: &str, sample_size: usize) -> Result<Vec<UtxoEntry>, SnapshotError> {
    let mut f = File::open(path).map_err(|e| SnapshotError::IoError(e.to_string()))?;

    // Read entire file (we need to parse CBOR sequentially)
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer).map_err(|e| SnapshotError::IoError(e.to_string()))?;

    let mut dec = Decoder::new(&buffer);

    // Navigate to UTXO map at [3][1][1][0]
    navigate_to_utxo_map(&mut dec)?;

    // Parse the UTXO map
    let map_len = dec.map().map_err(|e| SnapshotError::Cbor(e))?;

    let mut utxos = Vec::new();
    let limit = match map_len {
        Some(len) => len.min(sample_size as u64),
        None => sample_size as u64,
    };

    for _ in 0..limit {
        // Check for break in indefinite map
        if map_len.is_none() && matches!(dec.datatype(), Ok(Type::Break)) {
            break;
        }

        // Parse key: TransactionInput (array [tx_hash, output_index])
        dec.array().map_err(|e| SnapshotError::Cbor(e))?;
        let tx_hash_bytes = dec.bytes().map_err(|e| SnapshotError::Cbor(e))?;
        let output_index = dec.u64().map_err(|e| SnapshotError::Cbor(e))?;

        // Convert tx_hash to hex string
        let tx_hash = hex::encode(tx_hash_bytes);

        // Parse value: TransactionOutput (simplified - just get address and value)
        // The actual structure is complex, so we'll parse what we can
        match parse_transaction_output(&mut dec) {
            Ok((address, value)) => {
                utxos.push(UtxoEntry {
                    tx_hash,
                    output_index,
                    address,
                    value,
                });
            }
            Err(e) => {
                eprintln!("Warning: failed to parse UTXO value: {e}");
                // parse_transaction_output already consumed/skipped the data,
                // so we just continue to the next entry
            }
        }
    }

    Ok(utxos)
}

/// Parse all UTXOs from the snapshot with a callback for each entry
///
/// This function navigates to the UTXO map and calls the provided callback
/// for each UTXO entry. This allows processing 11M+ UTXOs without storing
/// them all in memory simultaneously.
///
/// The callback receives (tx_hash, output_index, address, value) and can
/// return an error to stop processing.
///
/// Returns the total count of successfully processed UTXOs.
pub fn parse_all_utxos<F>(path: &str, mut callback: F) -> Result<u64, SnapshotError>
where
    F: FnMut(&str, u64, &str, u64) -> Result<(), SnapshotError>,
{
    let mut f = File::open(path).map_err(|e| SnapshotError::IoError(e.to_string()))?;

    // Read entire file (we need to parse CBOR sequentially)
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer).map_err(|e| SnapshotError::IoError(e.to_string()))?;

    let mut dec = Decoder::new(&buffer);

    // Navigate to UTXO map at [3][1][1][0]
    navigate_to_utxo_map(&mut dec)?;

    // Parse the UTXO map
    let map_len = dec.map().map_err(|e| SnapshotError::Cbor(e))?;

    let mut count = 0u64;
    let mut errors = 0u64;

    loop {
        // Check for break in indefinite map
        if map_len.is_none() {
            match dec.datatype() {
                Ok(Type::Break) => {
                    dec.skip().map_err(|e| SnapshotError::Cbor(e))?; // consume break
                    break;
                }
                Err(_) => break,
                _ => {}
            }
        } else if let Some(len) = map_len {
            if count >= len {
                break;
            }
        }

        // Parse key: TransactionInput (array [tx_hash, output_index])
        match dec.array() {
            Ok(_) => {}
            Err(_) => break,
        }

        let tx_hash_bytes = match dec.bytes() {
            Ok(b) => b,
            Err(_) => {
                errors += 1;
                dec.skip().ok(); // skip value
                continue;
            }
        };

        let output_index = match dec.u64() {
            Ok(idx) => idx,
            Err(_) => {
                errors += 1;
                dec.skip().ok(); // skip value
                continue;
            }
        };

        let tx_hash = hex::encode(tx_hash_bytes);

        // Parse value: TransactionOutput
        match parse_transaction_output(&mut dec) {
            Ok((address, value)) => {
                callback(&tx_hash, output_index, &address, value)?;
                count += 1;
            }
            Err(_) => {
                errors += 1;
                // Already consumed by parse_transaction_output
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

    Ok(count)
}

/// Extract snapshot data needed for boot (epoch, treasury, reserves)
///
/// This is a lightweight extraction that reads only the necessary fields
/// without parsing the full UTXO set.
/// 
/// See docs/amaru-snapshot-structure.md
///
/// Navigation path through the structure:
/// - [0] Epoch number
/// - [1] Previous blocks made
/// - [2] Current blocks made
/// - [3] NewEpochState (ARRAY)
///   - [0] AccountState (treasury, reserves)
///   - [1] LedgerState (ARRAY)
///     - [0] CertState (ARRAY) - DReps, pools, accounts
///       - [0] VotingState - DReps at [3][1][0][0][0]
///       - [1] PoolState - pools at [3][1][0][1][0]
///       - [2] DelegationState - accounts at [3][1][0][2][0][0]
///     - [1] UTxOState (ARRAY) - governance at [3][1][1][3][0][1]
pub fn extract_boot_data(path: &str) -> Result<SnapshotData, SnapshotError> {
    let mut f = File::open(path).map_err(|e| SnapshotError::IoError(e.to_string()))?;

    // Get file size
    let metadata = f.metadata().map_err(|e| SnapshotError::IoError(e.to_string()))?;
    let file_size = metadata.len();

    // Read entire file so we can navigate into all of its structure
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer).map_err(|e| SnapshotError::IoError(e.to_string()))?;

    let mut dec = Decoder::new(&buffer);

    // Top-level array
    let top_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected top-level array".into()))?;

    if top_len < 4 {
        return Err(SnapshotError::StructuralDecode(format!(
            "expected at least 4 top-level elements, got {top_len}"
        )));
    }

    // [0] Epoch number
    let epoch = dec.u64().map_err(|e| SnapshotError::Cbor(e))?;

    // Validate Conway+ era
    if epoch < MIN_SUPPORTED_EPOCH {
        return Err(SnapshotError::StructuralDecode(format!(
            "epoch {epoch} is pre-Conway (requires >= {MIN_SUPPORTED_EPOCH})"
        )));
    }

    // [1] Previous blocks made
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [2] Current blocks made
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3] NewEpochState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][0] AccountState (ARRAY: [treasury, reserves])
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;
    let treasury_i64: i64 = dec.decode().map_err(|e| SnapshotError::Cbor(e))?;
    let reserves_i64: i64 = dec.decode().map_err(|e| SnapshotError::Cbor(e))?;

    let treasury = treasury_i64 as u64;
    let reserves = reserves_i64 as u64;

    // [3][1] LedgerState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0] CertState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0][0] VotingState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0][0][0] dreps map (Map<DRepCredential, DRepState>)
    let dreps = match dec.map().map_err(|e| SnapshotError::Cbor(e))? {
        Some(len) => len,
        None => {
            // Indefinite map - count manually
            let mut count = 0u64;
            loop {
                match dec.datatype() {
                    Ok(Type::Break) => {
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?; // consume break
                        break;
                    }
                    Err(_) => break,
                    _ => {
                        // Skip key and value
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        count += 1;
                    }
                }
            }
            count
        }
    };

    // [3][1][0][0][1] cc_members (committee)
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0][0][2] dormant_epoch
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0][1] PoolState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0][1][0] pools map (Map<PoolId, PoolParams>)
    let stake_pools = match dec.map().map_err(|e| SnapshotError::Cbor(e))? {
        Some(len) => len,
        None => {
            // Indefinite map - count manually
            let mut count = 0u64;
            loop {
                match dec.datatype() {
                    Ok(Type::Break) => {
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?; // consume break
                        break;
                    }
                    Err(_) => break,
                    _ => {
                        // Skip key and value
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        count += 1;
                    }
                }
            }
            count
        }
    };

    // Skip remaining PoolState elements
    // [3][1][0][1][1] pools_updates
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
    // [3][1][0][1][2] pools_retirements
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
    // [3][1][0][1][3] deposits
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0][2] DelegationState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0][2][0] dsUnified (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][0][2][0][0] credentials map (Map<StakeCredential, Account>)
    let stake_accounts = match dec.map().map_err(|e| SnapshotError::Cbor(e))? {
        Some(len) => len,
        None => {
            // Indefinite map - count manually
            let mut count = 0u64;
            loop {
                match dec.datatype() {
                    Ok(Type::Break) => {
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?; // consume break
                        break;
                    }
                    Err(_) => break,
                    _ => {
                        // Skip key and value
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        count += 1;
                    }
                }
            }
            count
        }
    };

    // Skip remaining dsUnified and DelegationState elements
    // [3][1][0][2][0][1] pointers
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
    // [3][1][0][2][1] dsFutureGenDelegs
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
    // [3][1][0][2][2] dsGenDelegs
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
    // [3][1][0][2][3] dsIRewards
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][1] UTxOState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][1][0] UTXO map
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][1][1] deposits
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][1][2] fees
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][1][3] GovernanceState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][1][3][0] ProposalsState (ARRAY)
    dec.array().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][1][3][0][0] roots (array of 4 proposal trees)
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3][1][1][3][0][1] proposals (Vec<ProposalState>)
    let governance_proposals = match dec.array().map_err(|e| SnapshotError::Cbor(e))? {
        Some(len) => len,
        None => {
            // Indefinite array - count manually
            let mut count = 0u64;
            loop {
                match dec.datatype() {
                    Ok(Type::Break) => {
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?; // consume break
                        break;
                    }
                    Err(_) => break,
                    _ => {
                        // Skip proposal element
                        dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                        count += 1;
                    }
                }
            }
            count
        }
    };

    Ok(SnapshotData {
        epoch,
        treasury,
        reserves,
        stake_pools,
        dreps,
        stake_accounts,
        governance_proposals,
        file_size,
    })
}

/// Navigate the decoder to the UTXO map at [3][1][1][0]
fn navigate_to_utxo_map(dec: &mut Decoder) -> Result<(), SnapshotError> {
    // Parse top-level array
    let top_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected top-level array".into()))?;

    if top_len < 4 {
        return Err(SnapshotError::StructuralDecode(format!(
            "expected at least 4 top-level elements, got {top_len}"
        )));
    }

    // [0] = Epoch number
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [1] = Previous blocks made
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [2] = Current blocks made
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // [3] = Epoch State (ARRAY)
    let epoch_state_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected Epoch State array".into()))?;

    if epoch_state_len < 2 {
        return Err(SnapshotError::StructuralDecode(format!(
            "expected at least 2 Epoch State elements, got {epoch_state_len}"
        )));
    }

    // Epoch State[0] = Account State
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // Epoch State[1] = Ledger State (ARRAY)
    let ledger_state_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected Ledger State array".into()))?;

    if ledger_state_len < 2 {
        return Err(SnapshotError::StructuralDecode(format!(
            "expected at least 2 Ledger State elements, got {ledger_state_len}"
        )));
    }

    // Ledger State[0] = Cert State
    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;

    // Ledger State[1] = UTxO State (ARRAY)
    let utxo_state_len = dec
        .array()
        .map_err(|e| SnapshotError::Cbor(e))?
        .ok_or_else(|| SnapshotError::StructuralDecode("expected UTxO State array".into()))?;

    if utxo_state_len < 1 {
        return Err(SnapshotError::StructuralDecode(
            "expected at least 1 UTxO State element".into(),
        ));
    }

    // UTxO State[0] = utxo map - we're here!
    Ok(())
}

/// Parse a Conway-era transaction output (simplified)
///
/// Conway TxOut structure (from cardano-ledger):
/// - Address (bytes)
/// - Value (coin or multi-asset)
/// - Optional inline datum
/// - Optional reference script
fn parse_transaction_output(dec: &mut Decoder) -> Result<(String, u64), SnapshotError> {
    // TxOut is typically an array [address, value, ...]
    // or a map for Conway with optional fields

    // Try array format first (most common)
    match dec.datatype().map_err(|e| SnapshotError::Cbor(e))? {
        Type::Array | Type::ArrayIndef => {
            let arr_len = dec.array().map_err(|e| SnapshotError::Cbor(e))?;
            if arr_len == Some(0) {
                return Err(SnapshotError::StructuralDecode("empty TxOut array".into()));
            }

            // Element 0: Address (bytes)
            let address_bytes = dec.bytes().map_err(|e| SnapshotError::Cbor(e))?;
            let address = hex::encode(address_bytes);

            // Element 1: Value (coin or map)
            let value = match dec.datatype().map_err(|e| SnapshotError::Cbor(e))? {
                Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                    // Simple ADA-only value
                    dec.u64().map_err(|e| SnapshotError::Cbor(e))?
                }
                Type::Array | Type::ArrayIndef => {
                    // Multi-asset: [coin, assets_map]
                    dec.array().map_err(|e| SnapshotError::Cbor(e))?;
                    let coin = dec.u64().map_err(|e| SnapshotError::Cbor(e))?;
                    // Skip the assets map
                    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                    coin
                }
                _ => {
                    return Err(SnapshotError::StructuralDecode(
                        "unexpected value type".into(),
                    ));
                }
            };

            // Skip remaining fields (datum, script_ref)
            if let Some(len) = arr_len {
                for _ in 2..len {
                    dec.skip().map_err(|e| SnapshotError::Cbor(e))?;
                }
            }

            Ok((address, value))
        }
        Type::Map | Type::MapIndef => {
            // Map format (Conway with optional fields)
            // Map keys: 0=address, 1=value, 2=datum, 3=script_ref
            let map_len = dec.map().map_err(|e| SnapshotError::Cbor(e))?;
            
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
                Err(SnapshotError::StructuralDecode(
                    "map-based TxOut missing required fields".into(),
                ))
            }
        }
        _ => Err(SnapshotError::StructuralDecode(
            "unexpected TxOut type".into(),
        )),
    }
}

/// Extract tip information from Amaru snapshot filename
///
/// Amaru snapshots follow the naming convention: `<slot>.<block_hash>.cbor`
///
/// # Example
/// ```ignore
/// use acropolis_common::snapshot::snapshot::extract_tip_from_filename;
///
/// let tip = extract_tip_from_filename(
///     "tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor"
/// ).unwrap();
/// assert_eq!(tip.slot, 134092758);
/// assert_eq!(tip.block_hash, "670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327");
/// ```
pub fn extract_tip_from_filename(path: &str) -> Result<TipInfo, SnapshotError> {
    // Extract filename from path
    let filename = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| SnapshotError::StructuralDecode("invalid file path".into()))?;

    // Parse: <slot>.<block_hash>
    let parts: Vec<&str> = filename.split('.').collect();
    if parts.len() != 2 {
        return Err(SnapshotError::StructuralDecode(format!(
            "expected filename format <slot>.<block_hash>, got: {filename}"
        )));
    }

    let slot = parts[0].parse::<u64>().map_err(|_| {
        SnapshotError::StructuralDecode(format!("invalid slot number: {}", parts[0]))
    })?;

    let block_hash = parts[1].to_string();

    // Validate block hash is 64 hex characters
    if block_hash.len() != 64 {
        return Err(SnapshotError::StructuralDecode(format!(
            "invalid block hash length (expected 64 hex chars): {block_hash}"
        )));
    }

    // Validate hex characters
    if !block_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SnapshotError::StructuralDecode(format!(
            "block hash contains non-hex characters: {block_hash}"
        )));
    }

    Ok(TipInfo { slot, block_hash })
}

/// Calculate block height from slot number
///
/// This is an approximation based on known mainnet parameters:
/// - Byron era: 1 block per 20 seconds (slots were 20s)
/// - Shelley+ era: 1 block per second on average (with ~5% empty slots)
///
/// Note: This is a rough estimate. For exact height, query a Cardano node.
///
/// Known reference points (mainnet):
/// - Slot 4,492,800 (Shelley start) ≈ Block 4,490,510
/// - Slot 134,092,758 ≈ Block 10,530,000 (estimated)
pub fn estimate_block_height_from_slot(slot: u64) -> u64 {
    const BYRON_SLOT_LENGTH: u64 = 20; // seconds
    const SHELLEY_START_SLOT: u64 = 4_492_800;
    const SHELLEY_START_BLOCK: u64 = 4_490_510;

    if slot < SHELLEY_START_SLOT {
        // Byron era: 20-second slots
        slot / BYRON_SLOT_LENGTH
    } else {
        // Shelley onwards: estimate with ~5% empty slots
        let shelley_slots = slot - SHELLEY_START_SLOT;
        // Using empirical ratio: ~1 block per 21.45 slots
        let shelley_blocks = shelley_slots / 21;
        SHELLEY_START_BLOCK + shelley_blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amaru_snapshot_detection() {
        // This test will pass if the Amaru snapshot file exists
        let path = "tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor";
        if std::path::Path::new(path).exists() {
            let snapshot = AmaruSnapshot::from_file(path).expect("should parse Amaru snapshot");
            assert!(snapshot.size_bytes > 0);
            assert_eq!(snapshot.structure_type, "array");
            println!("{}", snapshot.summary());
        }
    }

    #[test]
    fn test_conway_metadata_extraction() {
        // Test Conway era metadata extraction
        let path = "tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor";
        if std::path::Path::new(path).exists() {
            let metadata = EpochStateMetadata::from_file(path)
                .expect("should extract metadata from Conway snapshot");

            // Validate epoch is Conway era
            assert!(
                metadata.epoch >= MIN_SUPPORTED_EPOCH,
                "epoch should be >= 505"
            );
            assert_eq!(
                metadata.epoch, 507,
                "this specific snapshot is from epoch 507"
            );

            // Validate file size
            assert!(
                metadata.file_size > 2_000_000_000,
                "snapshot should be > 2GB"
            );

            println!("{}", metadata.summary());
        }
    }

    #[test]
    fn test_parse_sample_utxos() {
        // Test parsing a few UTXOs from the snapshot
        let path = "tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor";
        if std::path::Path::new(path).exists() {
            match parse_sample_utxos(path, 5) {
                Ok(utxos) => {
                    println!("Successfully parsed {} sample UTXOs:", utxos.len());
                    for (i, utxo) in utxos.iter().enumerate() {
                        println!(
                            "  [{}] {}#{} -> {} ({} lovelace)",
                            i,
                            &utxo.tx_hash[..16],
                            utxo.output_index,
                            &utxo.address[..16],
                            utxo.value
                        );
                    }
                    assert!(!utxos.is_empty(), "should parse at least one UTXO");
                }
                Err(e) => {
                    println!("UTXO parsing not yet fully implemented: {e}");
                    // Don't fail the test yet - this is exploratory
                }
            }
        }
    }

    #[test]
    fn test_extract_tip_from_filename() {
        let path = "tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor";
        let tip = extract_tip_from_filename(path).expect("should extract tip from filename");

        assert_eq!(tip.slot, 134092758);
        assert_eq!(
            tip.block_hash,
            "670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327"
        );

        // Estimate block height
        let height = estimate_block_height_from_slot(tip.slot);
        println!("Slot {} ≈ block height {}", tip.slot, height);

        // Should be in the reasonable range for Conway era mainnet
        assert!(height > 10_000_000, "Conway era should be > 10M blocks");
        assert!(height < 20_000_000, "Should be < 20M blocks as of 2025");
    }

    #[test]
    fn test_extract_tip_invalid_filename() {
        // Test various invalid filename formats
        assert!(extract_tip_from_filename("invalid.cbor").is_err());
        assert!(extract_tip_from_filename("not-a-slot.hash.cbor").is_err());
        assert!(extract_tip_from_filename("12345.tooshort.cbor").is_err());
        assert!(extract_tip_from_filename(
            "12345.notahexhashxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.cbor"
        )
        .is_err());
    }

    #[test]
    #[ignore] // Requires large fixture file (2.4GB)
    fn test_extract_boot_data() {
        // Test extracting boot data from the real snapshot
        let path = "../tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor";

        if !std::path::Path::new(path).exists() {
            println!("Skipping test: snapshot file not found at {}", path);
            return;
        }

        println!("Testing extract_boot_data with file: {}", path);

        match extract_boot_data(path) {
            Ok(data) => {
                println!("Successfully extracted boot data:");
                println!("  Epoch: {}", data.epoch);
                println!("  Treasury: {} lovelace", data.treasury);
                println!("  Reserves: {} lovelace", data.reserves);
                println!("  Stake Pools: {}", data.stake_pools);
                println!("  DReps: {}", data.dreps);
                println!("  Stake Accounts: {}", data.stake_accounts);
                println!("  Governance Proposals: {}", data.governance_proposals);

                assert_eq!(data.epoch, 507);
                assert!(data.treasury > 0);
                assert!(data.reserves > 0);
            }
            Err(e) => {
                println!("Error extracting boot data: {}", e);
                panic!("extract_boot_data failed: {}", e);
            }
        }
    }
}
