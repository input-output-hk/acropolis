use std::time::{Duration, SystemTime, UNIX_EPOCH};

use acropolis_common::genesis_values::GenesisValues;
use acropolis_common::Slot;
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::to_plutus_data::*;
use acropolis_common::validation::ScriptContextError;

/// POSIX time interval for transaction validity.
///
/// Constructed from slot-based validity intervals using GenesisValues
/// for slot-to-timestamp conversion.
pub struct TimeRange {
    pub lower_bound: Option<SystemTime>,
    pub upper_bound: Option<SystemTime>,
}

impl TimeRange {
    /// Create a new TimeRange by converting slot bounds to POSIX time
    /// using GenesisValues' slot-to-timestamp conversion.
    pub fn new(
        invalid_before: Option<Slot>,
        invalid_hereafter: Option<Slot>,
        genesis_values: &GenesisValues,
    ) -> Self {
        let slot_to_system_time = |slot: Slot| -> SystemTime {
            let timestamp_secs = genesis_values.slot_to_timestamp(slot);
            UNIX_EPOCH + Duration::from_secs(timestamp_secs)
        };

        Self {
            lower_bound: invalid_before.map(slot_to_system_time),
            upper_bound: invalid_hereafter.map(slot_to_system_time),
        }
    }
}

impl ToPlutusData for TimeRange {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let lower = {
            let (extended, closure) = match &self.lower_bound {
                Some(time) => {
                    let millis =
                        time.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
                    (
                        // Finite
                        constr(arena, 1, vec![millis.to_plutus_data(arena, version)?]),
                        // lower finite is inclusive: true
                        true.to_plutus_data(arena, version)?,
                    )
                }
                None => (
                    // NegInf
                    constr(arena, 0, vec![]),
                    // Infinite is always exclusive by convention: true
                    true.to_plutus_data(arena, version)?,
                ),
            };
            constr(arena, 0, vec![extended, closure])
        };

        let upper = {
            let (extended, closure) = match &self.upper_bound {
                Some(time) => {
                    let millis =
                        time.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
                    (
                        // Finite
                        constr(arena, 1, vec![millis.to_plutus_data(arena, version)?]),
                        // upper finite is exclusive: False,
                        false.to_plutus_data(arena, version)?,
                    )
                }
                None => (
                    // PosInf
                    constr(arena, 2, vec![]),
                    // Infinite is always exclusive by convention: true
                    true.to_plutus_data(arena, version)?,
                ),
            };
            constr(arena, 0, vec![extended, closure])
        };

        Ok(constr(arena, 0, vec![lower, upper]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::genesis_values::GenesisValues;

    #[test]
    fn time_range_shelley_slot() {
        let gv = GenesisValues::mainnet();
        // Mainnet: byron_timestamp=1506203091, shelley_epoch=208
        // Shelley start slot = 208 * 21600 = 4492800
        // Shelley start timestamp = 1506203091 + 4492800 * 20 = 1596059091
        // Slot 44_000_000 (post-Shelley): 1596059091 + (44_000_000 - 4_492_800) = 1635566291
        let tr = TimeRange::new(Some(44_000_000), Some(44_100_000), &gv);

        let lower_secs = tr.lower_bound.unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let upper_secs = tr.upper_bound.unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs();

        assert_eq!(lower_secs, 1_635_566_291);
        assert_eq!(upper_secs, 1_635_666_291);
        assert_eq!(upper_secs - lower_secs, 100_000); // 100k slots = 100k seconds in Shelley
    }

    #[test]
    fn time_range_none_bounds() {
        let gv = GenesisValues::mainnet();
        let tr = TimeRange::new(None, None, &gv);
        assert!(tr.lower_bound.is_none());
        assert!(tr.upper_bound.is_none());
    }

    #[test]
    fn time_range_mixed_bounds() {
        let gv = GenesisValues::mainnet();
        let tr = TimeRange::new(Some(44_000_000), None, &gv);
        assert!(tr.lower_bound.is_some());
        assert!(tr.upper_bound.is_none());
    }
}
