//! Common calculations for Cardano

const BYRON_SLOTS_PER_EPOCH: u64 = 21_600;
pub const SHELLEY_SLOTS_PER_EPOCH: u64 = 432_000;
const SHELLEY_START_SLOT: u64 = 4_492_800;
const SHELLEY_START_EPOCH: u64 = 208;

/// Derive an epoch number from a slot, handling Byron/Shelley era changes
pub fn slot_to_epoch(slot: u64) -> u64 {
    if slot < SHELLEY_START_SLOT {
        slot / BYRON_SLOTS_PER_EPOCH
    } else {
        SHELLEY_START_EPOCH + (slot - SHELLEY_START_SLOT) / SHELLEY_SLOTS_PER_EPOCH
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byron_epoch_0() {
        assert_eq!(0, slot_to_epoch(0));
    }

    #[test]
    fn byron_epoch_1() {
        assert_eq!(1, slot_to_epoch(21_600));
    }

    #[test]
    fn byron_last_slot() {
        assert_eq!(slot_to_epoch(4_492_799), 207);
    }

    #[test]
    fn shelley_first_slot() {
        assert_eq!(slot_to_epoch(4_492_800), 208);
    }

    #[test]
    fn shelley_epoch_209_start() {
        // 432_000 slots later
        assert_eq!(slot_to_epoch(4_492_800 + 432_000), 209);
    }

    #[test]
    fn before_transition_boundary() {
        // One slot before Shelley starts
        assert_eq!(slot_to_epoch(4_492_799), 207);
    }

    #[test]
    fn after_transition_boundary() {
        // First Shelley slot
        assert_eq!(slot_to_epoch(4_492_800), 208);
    }

    #[test]
    fn mainnet_example_from_cexplorer() {
        // Slot 98_272_003 maps to epoch 425
        assert_eq!(slot_to_epoch(98_272_003), 425);
    }
}
