//! Era history types and slot/epoch/time conversion utilities.
//!
//! Provides [`EraHistory`] — a complete history of Cardano protocol eras for a
//! network — along with utility methods for era-aware slot-to-epoch,
//! epoch-to-slot, and slot-to-POSIX-time conversions.

use crate::{era_summary::EraSummary, types::Era, Epoch, Slot};
use std::time::{Duration, SystemTime};

/// Complete era history for a network, including system start time.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EraHistory {
    /// Network stability window in slots (3k/f).
    pub stability_window: u64,
    /// Ordered array of era summaries (Byron first, current era last).
    pub eras: Vec<EraSummary>,
    /// System start time as a POSIX timestamp. Set at runtime from GenesisValues;
    pub system_start: SystemTime,
}

/// Error type for era history operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EraHistoryError {
    /// The requested slot falls before the start of the era history.
    #[error("slot {0} is before era history start")]
    SlotBeforeHistory(u64),
    /// The requested slot is beyond the known era history.
    #[error("slot {0} is past the time horizon")]
    PastTimeHorizon(u64),
    /// The era history data is structurally invalid.
    #[error("invalid era history: {0}")]
    InvalidEraHistory(String),
    /// The slot is too far in the future.
    #[error("slot {0} is too far in the future")]
    SlotTooFar(u64),
}

impl EraHistory {
    /// Validate that era summaries are ordered, contiguous, and well-formed.
    pub fn validate(&self) -> Result<(), EraHistoryError> {
        if self.eras.is_empty() {
            return Err(EraHistoryError::InvalidEraHistory(
                "era history must not be empty".to_string(),
            ));
        }

        for (i, era) in self.eras.iter().enumerate() {
            if era.params.epoch_size_slots == 0 {
                return Err(EraHistoryError::InvalidEraHistory(format!(
                    "era {i} has zero epoch_size_slots"
                )));
            }
            if era.params.slot_length.is_zero() {
                return Err(EraHistoryError::InvalidEraHistory(format!(
                    "era {i} has zero slot_length"
                )));
            }

            if i > 0 {
                let prev = &self.eras[i - 1];
                let prev_end = prev.end.as_ref().ok_or_else(|| {
                    EraHistoryError::InvalidEraHistory(format!(
                        "era {prev_idx} has no end bound but is not the last era",
                        prev_idx = i - 1
                    ))
                })?;
                if era.start.slot != prev_end.slot || era.start.epoch != prev_end.epoch {
                    return Err(EraHistoryError::InvalidEraHistory(format!(
                        "era {i} start does not match era {} end",
                        i - 1
                    )));
                }
            }
        }

        let last = &self.eras[self.eras.len() - 1];
        if last.end.is_some() {
            return Err(EraHistoryError::InvalidEraHistory(
                "last era must have end = None".to_string(),
            ));
        }

        Ok(())
    }

    pub fn slot_to_posix_time(&self, slot: Slot, tip: Slot) -> Result<SystemTime, EraHistoryError> {
        let relative_time = self.slot_to_relative_time(slot, tip)?;

        Ok(self.system_start + relative_time)
    }

    pub fn slot_to_relative_time(
        &self,
        slot: Slot,
        tip: Slot,
    ) -> Result<Duration, EraHistoryError> {
        for era in &self.eras {
            if era.start.slot > slot {
                return Err(EraHistoryError::SlotBeforeHistory(slot));
            }

            if era.contains_slot(slot, tip, self.stability_window) {
                return slot_to_relative_time(slot, era);
            }
        }

        Err(EraHistoryError::PastTimeHorizon(slot))
    }

    pub fn slot_to_epoch(&self, slot: Slot, tip: Slot) -> Result<Epoch, EraHistoryError> {
        for era in &self.eras {
            if era.start.slot > slot {
                return Err(EraHistoryError::SlotBeforeHistory(slot));
            }

            if era.contains_slot(slot, tip, self.stability_window) {
                return slot_to_epoch(slot, era);
            }
        }

        Err(EraHistoryError::PastTimeHorizon(slot))
    }

    pub fn slot_to_epoch_unchecked_horizon(&self, slot: Slot) -> Result<Epoch, EraHistoryError> {
        for era in &self.eras {
            if era.start.slot > slot {
                return Err(EraHistoryError::SlotBeforeHistory(slot));
            }

            if era.contains_slot_unchecked_horizon(slot) {
                return slot_to_epoch(slot, era);
            }
        }

        Err(EraHistoryError::PastTimeHorizon(slot))
    }

    /// Determine which protocol era the given slot belongs to.
    pub fn slot_to_era(&self, slot: Slot) -> Result<Era, EraHistoryError> {
        for era in self.eras.iter() {
            if era.start.slot > slot {
                return Err(EraHistoryError::SlotBeforeHistory(slot));
            }

            if era.contains_slot_unchecked_horizon(slot) {
                return Ok(era.params.era_name);
            }
        }

        Err(EraHistoryError::PastTimeHorizon(slot))
    }
}

/// Compute the time in milliseconds between the start of the system and the given slot.
///
/// **pre-condition**: the given summary must be the era containing that slot.
fn slot_to_relative_time(slot: Slot, era: &EraSummary) -> Result<Duration, EraHistoryError> {
    let slots_elapsed: u32 = match slot.checked_sub(era.start.slot) {
        Some(elapsed) => elapsed.try_into().map_err(|_| EraHistoryError::SlotTooFar(slot))?,
        None => {
            return Err(EraHistoryError::InvalidEraHistory(format!(
                "pre-condition not met: slot {slot} is not in era {}",
                era.params.era_name
            )))
        }
    };

    let time_elapsed = era.params.slot_length * slots_elapsed;

    let relative_time = era.start.time + time_elapsed;

    Ok(relative_time)
}

/// Compute the epoch corresponding to the given slot.
///
/// **pre-condition**: the given summary must be the era containing that slot.
fn slot_to_epoch(slot: Slot, era: &EraSummary) -> Result<Epoch, EraHistoryError> {
    let slots_elapsed = slot.checked_sub(era.start.slot).ok_or_else(|| {
        EraHistoryError::InvalidEraHistory(format!(
            "pre-condition not met: slot {slot} is not in era {}",
            era.params.era_name
        ))
    })?;
    let epochs_elapsed = slots_elapsed / era.params.epoch_size_slots;
    let epoch_number = era.start.epoch + epochs_elapsed;
    Ok(epoch_number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::era_summary::{EraBound, EraParams};

    const MAINNET_SYSTEM_START_SECS: u64 = 1_506_203_091;

    fn mainnet_system_start() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(MAINNET_SYSTEM_START_SECS)
    }

    fn mainnet_era_history() -> EraHistory {
        let json = include_str!("../../modules/era_state/data/mainnet-era-history.json");
        serde_json::from_str(json).expect("mainnet era history JSON is valid")
    }

    /// A tip far enough in the future that all mainnet test slots are reachable.
    const FAR_TIP: Slot = 200_000_000;

    #[test]
    fn mainnet_json_parses_and_validates() {
        let history = mainnet_era_history();
        assert_eq!(history.eras.len(), 7);
        assert_eq!(history.stability_window, 129_600);
        history.validate().expect("mainnet era history is valid");
    }

    #[test]
    fn mainnet_first_era_is_byron() {
        let history = mainnet_era_history();
        assert_eq!(history.eras[0].params.era_name, Era::Byron);
        assert_eq!(history.eras[0].params.epoch_size_slots, 21_600);
        assert_eq!(history.eras[0].params.slot_length, Duration::from_secs(20));
    }

    #[test]
    fn mainnet_last_era_is_conway() {
        let history = mainnet_era_history();
        let last = history.eras.last().expect("eras not empty");
        assert_eq!(last.params.era_name, Era::Conway);
        assert!(last.end.is_none());
    }

    // --- slot_to_epoch ---

    #[test]
    fn slot_to_epoch_byron_slot_0() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_epoch_unchecked_horizon(0).unwrap(), 0);
    }

    #[test]
    fn slot_to_epoch_byron_epoch_1() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_epoch_unchecked_horizon(21_600).unwrap(), 1);
    }

    #[test]
    fn slot_to_epoch_byron_last_slot() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_epoch_unchecked_horizon(4_492_799).unwrap(), 207);
    }

    #[test]
    fn slot_to_epoch_shelley_first_slot() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_epoch_unchecked_horizon(4_492_800).unwrap(), 208);
    }

    #[test]
    fn slot_to_epoch_conway_start() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_epoch_unchecked_horizon(133_660_800).unwrap(), 507);
    }

    #[test]
    fn slot_to_epoch_cexplorer_reference() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_epoch_unchecked_horizon(98_272_003).unwrap(), 425);
    }

    // --- slot_to_posix_time ---

    #[test]
    fn slot_to_posix_time_slot_0() {
        let h = mainnet_era_history();
        assert_eq!(
            h.slot_to_posix_time(0, FAR_TIP).unwrap(),
            mainnet_system_start()
        );
    }

    #[test]
    fn slot_to_posix_time_byron_slot() {
        let h = mainnet_era_history();
        let expected = mainnet_system_start() + Duration::from_secs(21_600 * 20);
        assert_eq!(h.slot_to_posix_time(21_600, FAR_TIP).unwrap(), expected);
    }

    #[test]
    fn slot_to_posix_time_shelley_first_slot() {
        let h = mainnet_era_history();
        let expected = mainnet_system_start() + Duration::from_secs(89_856_000);
        assert_eq!(h.slot_to_posix_time(4_492_800, FAR_TIP).unwrap(), expected);
    }

    #[test]
    fn slot_to_posix_time_matches_existing_calculation() {
        let h = mainnet_era_history();
        let posix = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        assert_eq!(
            h.slot_to_posix_time(0, FAR_TIP).unwrap(),
            posix(1_506_203_091)
        );
        assert_eq!(
            h.slot_to_posix_time(21_600, FAR_TIP).unwrap(),
            posix(1_506_635_091)
        );
        assert_eq!(
            h.slot_to_posix_time(4_492_800, FAR_TIP).unwrap(),
            posix(1_596_059_091)
        );
        assert_eq!(
            h.slot_to_posix_time(4_492_800 + 432_000, FAR_TIP).unwrap(),
            posix(1_596_491_091)
        );
        assert_eq!(
            h.slot_to_posix_time(98_272_003, FAR_TIP).unwrap(),
            posix(1_689_838_294)
        );
    }

    // --- slot_to_era ---

    #[test]
    fn slot_to_era_byron() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_era(0).unwrap(), Era::Byron);
        assert_eq!(h.slot_to_era(4_492_799).unwrap(), Era::Byron);
    }

    #[test]
    fn slot_to_era_shelley() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_era(4_492_800).unwrap(), Era::Shelley);
    }

    #[test]
    fn slot_to_era_conway() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_era(133_660_800).unwrap(), Era::Conway);
        assert_eq!(h.slot_to_era(200_000_000).unwrap(), Era::Conway);
    }

    // --- slot_to_relative_time ---

    #[test]
    fn slot_to_relative_time_slot_0() {
        let h = mainnet_era_history();
        assert_eq!(h.slot_to_relative_time(0, FAR_TIP).unwrap(), Duration::ZERO);
    }

    #[test]
    fn slot_to_relative_time_byron_slot() {
        let h = mainnet_era_history();
        assert_eq!(
            h.slot_to_relative_time(21_600, FAR_TIP).unwrap(),
            Duration::from_secs(21_600 * 20)
        );
    }

    #[test]
    fn slot_to_relative_time_shelley_first_slot() {
        let h = mainnet_era_history();
        assert_eq!(
            h.slot_to_relative_time(4_492_800, FAR_TIP).unwrap(),
            Duration::from_secs(89_856_000)
        );
    }

    // --- validate ---

    #[test]
    fn validate_empty_history() {
        let h = EraHistory {
            stability_window: 100,
            eras: vec![],
            system_start: SystemTime::UNIX_EPOCH,
        };
        assert!(h.validate().is_err());
    }

    #[test]
    fn validate_last_era_has_end() {
        let h = EraHistory {
            stability_window: 100,
            eras: vec![EraSummary {
                start: EraBound {
                    time: Duration::from_secs(0),
                    slot: 0,
                    epoch: 0,
                },
                end: Some(EraBound {
                    time: Duration::from_secs(100),
                    slot: 100,
                    epoch: 1,
                }),
                params: EraParams {
                    era_name: Era::Byron,
                    epoch_size_slots: 100,
                    slot_length: Duration::from_secs(1),
                },
            }],
            system_start: SystemTime::UNIX_EPOCH,
        };
        assert!(h.validate().is_err());
    }

    #[test]
    fn validate_non_contiguous_eras() {
        let h = EraHistory {
            stability_window: 100,
            eras: vec![
                EraSummary {
                    start: EraBound {
                        time: Duration::from_secs(0),
                        slot: 0,
                        epoch: 0,
                    },
                    end: Some(EraBound {
                        time: Duration::from_secs(100),
                        slot: 100,
                        epoch: 1,
                    }),
                    params: EraParams {
                        era_name: Era::Byron,
                        epoch_size_slots: 100,
                        slot_length: Duration::from_secs(1),
                    },
                },
                EraSummary {
                    start: EraBound {
                        time: Duration::from_secs(200),
                        slot: 200,
                        epoch: 2,
                    },
                    end: None,
                    params: EraParams {
                        era_name: Era::Shelley,
                        epoch_size_slots: 100,
                        slot_length: Duration::from_secs(1),
                    },
                },
            ],
            system_start: SystemTime::UNIX_EPOCH,
        };
        assert!(h.validate().is_err());
    }

    #[test]
    fn validate_zero_epoch_size() {
        let h = EraHistory {
            stability_window: 100,
            eras: vec![EraSummary {
                start: EraBound {
                    time: Duration::from_secs(0),
                    slot: 0,
                    epoch: 0,
                },
                end: None,
                params: EraParams {
                    era_name: Era::Conway,
                    epoch_size_slots: 0,
                    slot_length: Duration::from_secs(1),
                },
            }],
            system_start: SystemTime::UNIX_EPOCH,
        };
        assert!(h.validate().is_err());
    }

    // --- preprod and preview ---

    #[test]
    fn preprod_json_parses_and_validates() {
        let json = include_str!("../../modules/era_state/data/preprod-era-history.json");
        let history: EraHistory =
            serde_json::from_str(json).expect("preprod era history JSON is valid");
        assert_eq!(history.eras.len(), 7);
        history.validate().expect("preprod era history is valid");
    }

    #[test]
    fn preprod_slot_to_epoch() {
        let json = include_str!("../../modules/era_state/data/preprod-era-history.json");
        let history: EraHistory = serde_json::from_str(json).unwrap();
        assert_eq!(history.slot_to_epoch_unchecked_horizon(86_400).unwrap(), 4);
        assert_eq!(
            history.slot_to_epoch_unchecked_horizon(3_542_399).unwrap(),
            11
        );
        assert_eq!(
            history.slot_to_epoch_unchecked_horizon(3_542_400).unwrap(),
            12
        );
    }

    #[test]
    fn preview_json_parses_and_validates() {
        let json = include_str!("../../modules/era_state/data/preview-era-history.json");
        let history: EraHistory =
            serde_json::from_str(json).expect("preview era history JSON is valid");
        assert_eq!(history.eras.len(), 7);
    }

    #[test]
    fn sanchonet_json_parses_and_validates() {
        let json = include_str!("../../modules/era_state/data/sanchonet-era-history.json");
        let history: EraHistory =
            serde_json::from_str(json).expect("sanchonet era history JSON is valid");
        assert_eq!(history.eras.len(), 1);
        assert_eq!(history.eras[0].params.era_name, Era::Conway);
        history.validate().expect("sanchonet era history is valid");
    }
}
