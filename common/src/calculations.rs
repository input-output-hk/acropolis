//! Common calculations for Cardano

const BYRON_SLOTS_PER_EPOCH: u64 = 21_600;
pub const SHELLEY_SLOTS_PER_EPOCH: u64 = 432_000;
const SHELLEY_START_EPOCH: u64 = 208;
const BYRON_START_TIMESTAMP: u64 = 1506203091;

/// Derive an epoch number from a slot, handling Byron/Shelley era changes
pub fn slot_to_epoch(slot: u64) -> u64 {
    slot_to_epoch_with_shelley_params(slot, SHELLEY_START_EPOCH, SHELLEY_SLOTS_PER_EPOCH)
}

pub fn slot_to_epoch_with_shelley_params(
    slot: u64,
    shelley_epoch: u64,
    shelley_epoch_len: u64,
) -> u64 {
    let shelley_start_slot = shelley_epoch * BYRON_SLOTS_PER_EPOCH;
    if slot < shelley_start_slot {
        slot / BYRON_SLOTS_PER_EPOCH
    } else {
        shelley_epoch + (slot - shelley_start_slot) / shelley_epoch_len
    }
}

pub fn slot_to_timestamp(slot: u64) -> u64 {
    slot_to_timestamp_with_params(slot, BYRON_START_TIMESTAMP, SHELLEY_START_EPOCH)
}

pub fn slot_to_timestamp_with_params(
    slot: u64,
    byron_timestamp: u64,
    shelley_epoch: u64,
) -> u64 {
    let shelley_start_slot = shelley_epoch * BYRON_SLOTS_PER_EPOCH;
    if slot < shelley_start_slot {
        byron_timestamp + slot * 20
    } else {
        let shelley_timestamp = byron_timestamp + shelley_start_slot * 20;
        shelley_timestamp + (slot - shelley_start_slot)
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byron_epoch_0() {
        assert_eq!(slot_to_epoch(0), 0);
        assert_eq!(slot_to_timestamp(0), 1506203091);
    }

    #[test]
    fn byron_epoch_1() {
        assert_eq!(slot_to_epoch(21_600), 1);
        assert_eq!(slot_to_timestamp(21_600), 1506635091);
    }

    #[test]
    fn byron_last_slot() {
        assert_eq!(slot_to_epoch(4_492_799), 207);
        assert_eq!(slot_to_timestamp(4_492_799), 1596059071);
    }

    #[test]
    fn shelley_first_slot() {
        assert_eq!(slot_to_epoch(4_492_800), 208);
        assert_eq!(slot_to_timestamp(4_492_800), 1596059091);
    }

    #[test]
    fn shelley_epoch_209_start() {
        // 432_000 slots later
        assert_eq!(slot_to_epoch(4_492_800 + 432_000), 209);
        assert_eq!(slot_to_timestamp(4_492_800 + 432_000), 1596491091);
    }

    #[test]
    fn mainnet_example_from_cexplorer() {
        // Slot 98_272_003 maps to epoch 425
        assert_eq!(slot_to_epoch(98_272_003), 425);
        assert_eq!(slot_to_timestamp(98_272_003), 1689838294);
    }
}
