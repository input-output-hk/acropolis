//! Ouroboros overlay schedule
//! This is to validate the blocks which are reserved for Genesis Keys.
//!
//! Reference: https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L318
//!
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/Overlay.hs#L332

use acropolis_common::{
    rational_number::RationalNumber, GenesisDelegate, GenesisDelegates, GenesisKeyhash,
};
use anyhow::Result;
use num_traits::ToPrimitive;

#[derive(Debug, Clone, PartialEq)]
pub enum OBftSlot {
    /// Overlay slot but no block should be produced (rare edge case)
    NonActiveSlot,
    /// Active overlay slot reserved for specific genesis key
    ActiveSlot(GenesisKeyhash, GenesisDelegate),
}

/// Determine if the given slot is reserved for the overlay schedule.
///
/// # Arguments
/// * `epoch_slot` - The slot number delta of the block in the current epoch
///   (i.e. block's slot number - epoch's first slot number)
/// * `decentralisation_param` - The decentralization parameter
///
/// # Returns
/// `true` if the slot is reserved for the overlay schedule
///
/// If the slot is an overlay slot, then we skip StakeThreshold validation
/// since this block is produced by genesis key (without "lottery")
/// https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L334
pub fn is_overlay_slot(epoch_slot: u64, decentralisation_param: &RationalNumber) -> Result<bool> {
    let d = decentralisation_param
        .to_f64()
        .ok_or_else(|| anyhow::anyhow!("Failed to convert decentralisation parameter to f64"))?;

    // step function: ceiling of (x * d)
    let step = |x: f64| (x * d).ceil() as i64;

    Ok(step(epoch_slot as f64) < step((epoch_slot as f64) + 1.0))
}

/// Classify a slot in the overlay schedule, determining which genesis node
/// should produce the block if it's an active overlay slot.
///
/// # Arguments
/// * `epoch_slot` - The slot number delta of the block in the current epoch
/// * `genesis_delegs` - Set of genesis node key hashes and their delegations
/// * `decentralisation_param` - The decentralization parameter
/// * `active_slots_coeff` - The active slot coefficient
///
/// # Returns
/// Classification of the slot (NonActiveSlot or ActiveSlot with genesis key)
pub fn classify_overlay_slot(
    epoch_slot: u64,
    genesis_delegs: &GenesisDelegates,
    decentralisation_param: &RationalNumber,
    active_slots_coeff: &RationalNumber,
) -> Result<OBftSlot> {
    let d = decentralisation_param
        .to_f64()
        .ok_or_else(|| anyhow::anyhow!("Failed to convert decentralisation parameter to f64"))?;
    let position = (epoch_slot as f64 * d).ceil() as i64;

    // Calculate active slot coefficient inverse
    let asc_inv = active_slots_coeff
        .recip()
        .to_f64()
        .ok_or_else(|| anyhow::anyhow!("Failed to convert active slots coefficient to f64"))?
        .floor() as i64;

    let is_active = position % asc_inv == 0;

    if is_active {
        let genesis_idx = ((position / asc_inv) % genesis_delegs.as_ref().len() as i64) as usize;

        // Get the element at index from the set
        let (key_hash, gen_deleg) = genesis_delegs.as_ref().iter().nth(genesis_idx).unwrap();
        Ok(OBftSlot::ActiveSlot(*key_hash, gen_deleg.clone()))
    } else {
        Ok(OBftSlot::NonActiveSlot)
    }
}

/// Look up a slot in the overlay schedule to determine if it's reserved
/// and, if so, which genesis node should produce the block.
///
/// # Arguments
/// * `epoch_slot` - The slot number delta of the block in the current epoch
/// * `genesis_delegs` - Set of genesis node key hashes and their delegations
/// * `decentralisation_param` - The decentralization parameter
/// * `active_slots_coeff` - The active slot coefficient
///
/// # Returns
/// * `Some(OBftSlot)` if the slot is in the overlay schedule
/// * `None` if the slot is not in the overlay schedule
///
/// # Panics
/// `ShelleyParamsError` if:
/// - decentralisation_param is not a valid rational number
/// - active_slots_coeff is not a valid rational number
pub fn lookup_in_overlay_schedule(
    epoch_slot: u64,
    genesis_delegs: &GenesisDelegates,
    decentralisation_param: &RationalNumber,
    active_slots_coeff: &RationalNumber,
) -> Result<Option<OBftSlot>> {
    let is_overlay_slot = is_overlay_slot(epoch_slot, decentralisation_param)?;
    if is_overlay_slot {
        if genesis_delegs.as_ref().is_empty() {
            return Ok(None);
        }
        let obft_slot = classify_overlay_slot(
            epoch_slot,
            genesis_delegs,
            decentralisation_param,
            active_slots_coeff,
        )?;
        Ok(Some(obft_slot))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::genesis_values::GenesisValues;

    use super::*;

    #[test]
    fn test_lookup_in_overlay_schedule_1() {
        let genesis_values = GenesisValues::mainnet();
        let genesis_delegs = genesis_values.genesis_delegs;
        let decentralisation_param = RationalNumber::ONE;
        let active_slots_coeff = RationalNumber::new(1, 20);
        let epoch_slot = 0;
        let obft_slot = lookup_in_overlay_schedule(
            epoch_slot,
            &genesis_delegs,
            &decentralisation_param,
            &active_slots_coeff,
        )
        .unwrap();
        assert!(obft_slot.is_some());
        assert_eq!(
            obft_slot.unwrap(),
            OBftSlot::ActiveSlot(
                *genesis_delegs.as_ref().keys().next().unwrap(),
                genesis_delegs.as_ref().values().next().unwrap().clone()
            )
        );
    }

    #[test]
    fn test_lookup_in_overlay_schedule_2() {
        let genesis_values = GenesisValues::mainnet();
        let genesis_delegs = genesis_values.genesis_delegs;
        let decentralisation_param = RationalNumber::new(1, 2);
        let active_slots_coeff = RationalNumber::new(1, 20);
        let epoch_slot = 1;
        let obft_slot = lookup_in_overlay_schedule(
            epoch_slot,
            &genesis_delegs,
            &decentralisation_param,
            &active_slots_coeff,
        )
        .unwrap();
        assert!(obft_slot.is_none());
    }
}
