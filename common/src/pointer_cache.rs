//! Predefined pointer cache data for resolving Shelley pointer addresses
//! to stake addresses. This is used by both `stake_delta_filter` (for
//! ongoing stake delta resolution) and `accounts_state` (for subtracting
//! pointer address stake at the Conway boundary per spec 9.1.2).

use crate::{ShelleyAddressPointer, StakeAddress};
use anyhow::{anyhow, Result};
use serde_with::serde_as;
use std::collections::HashMap;

/// Predefined pointer cache: maps pointers to optional stake addresses.
/// Serialization uses a Vec of pairs so that HashMap keys are JSON-friendly.
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PredefinedPointerCache {
    #[serde_as(as = "Vec<(_, _)>")]
    pub pointer_map: HashMap<ShelleyAddressPointer, Option<StakeAddress>>,
    pub conway_start_slot: Option<u64>,
    pub max_slot: u64,
}

impl PredefinedPointerCache {
    /// Load a predefined pointer cache by network name (e.g. "Mainnet").
    pub fn load(name: &str) -> Result<Self> {
        let value = POINTER_CACHE
            .iter()
            .find_map(|(id, val)| if *id == name { Some(*val) } else { None })
            .ok_or_else(|| anyhow!("No predefined pointer cache for {name}"))?;

        serde_json::from_str::<PredefinedPointerCache>(value)
            .map_err(|e| anyhow!("Error parsing predefined pointer cache for {name}: {e}"))
    }

    /// Resolve a pointer to an optional stake address.
    pub fn resolve(&self, ptr: &ShelleyAddressPointer) -> Option<&StakeAddress> {
        self.pointer_map.get(ptr).and_then(|opt| opt.as_ref())
    }

    /// Resolve a map of pointer -> lovelace values into a map of
    /// stake_address -> lovelace, summing values where multiple pointers
    /// resolve to the same stake address. Pointers that cannot be resolved
    /// (no entry or null mapping) are skipped.
    pub fn resolve_to_stake_addresses(
        &self,
        pointer_values: &HashMap<ShelleyAddressPointer, u64>,
    ) -> HashMap<StakeAddress, u64> {
        let mut result: HashMap<StakeAddress, u64> = HashMap::new();
        for (ptr, lovelace) in pointer_values {
            if let Some(stake_addr) = self.resolve(ptr) {
                *result.entry(stake_addr.clone()).or_insert(0) += lovelace;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Built-in predefined data (network name, JSON)
// ---------------------------------------------------------------------------

pub const POINTER_CACHE: [(&str, &str); 1] = [(
    "Mainnet",
    r#"
{
  "pointer_map": [
    [
      {
        "slot": 18446744073709551615,
        "tx_index": 1221092,
        "cert_index": 2
      },
      {
        "network": "Mainnet",
        "credential": {
          "AddrKeyHash": "1332d859dd71f5b1089052a049690d81f7367eac9fafaef80b4da395"
        }
      }
    ],
    [
      {
        "slot": 16292793057,
        "tx_index": 1011302,
        "cert_index": 20
      },
      null
    ],
    [
      {
        "slot": 124,
        "tx_index": 21,
        "cert_index": 3807
      },
      null
    ],
    [
      {
        "slot": 222624,
        "tx_index": 45784521,
        "cert_index": 167387965
      },
      null
    ],
    [
      {
        "slot": 105,
        "tx_index": 13146,
        "cert_index": 24
      },
      null
    ],
    [
      {
        "slot": 62,
        "tx_index": 96,
        "cert_index": 105
      },
      null
    ],
    [
      {
        "slot": 4495800,
        "tx_index": 11,
        "cert_index": 0
      },
      {
        "network": "Mainnet",
        "credential": {
          "AddrKeyHash": "bc1597ad71c55d2d009a9274b3831ded155118dd769f5376decc1369"
        }
      }
    ],
    [
      {
        "slot": 100,
        "tx_index": 2,
        "cert_index": 0
      },
      null
    ],
    [
      {
        "slot": 53004562,
        "tx_index": 9,
        "cert_index": 0
      },
      {
        "network": "Mainnet",
        "credential": {
          "AddrKeyHash": "e46c33afa9ca60cfeb3b7452a415c271772020b3f57ac90c496a6127"
        }
      }
    ],
    [
      {
        "slot": 2498243,
        "tx_index": 27,
        "cert_index": 3
      },
      null
    ],
    [
      {
        "slot": 13005,
        "tx_index": 15312,
        "cert_index": 1878946283
      },
      null
    ],
    [
      {
        "slot": 12,
        "tx_index": 12,
        "cert_index": 12
      },
      null
    ],
    [
      {
        "slot": 20095460,
        "tx_index": 2,
        "cert_index": 0
      },
      {
        "network": "Mainnet",
        "credential": {
          "AddrKeyHash": "1332d859dd71f5b1089052a049690d81f7367eac9fafaef80b4da395"
        }
      }
    ],
    [
      {
        "slot": 13200,
        "tx_index": 526450,
        "cert_index": 149104513
      },
      null
    ],
    [
      {
        "slot": 116,
        "tx_index": 49,
        "cert_index": 0
      },
      null
    ]
  ],
  "conway_start_slot": 133660855,
  "max_slot": 154396745
}
"#,
)];
