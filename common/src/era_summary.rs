use crate::{types::Era, Slot};
use serde_with::serde_as;
use std::time::Duration;

/// A point marking an era boundary.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EraBound {
    /// Wall-clock time offset from system start.
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    #[serde(rename = "time_secs")]
    pub time: Duration,
    /// Absolute slot number at this boundary.
    pub slot: u64,
    /// Epoch number at this boundary.
    pub epoch: u64,
}

/// Parameters for a single protocol era.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EraParams {
    /// Which protocol era this describes.
    pub era_name: Era,
    /// Number of slots in each epoch for this era.
    pub epoch_size_slots: u64,
    /// Duration of each slot.
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    #[serde(rename = "slot_length_secs")]
    pub slot_length: Duration,
}

/// Metadata for a single era, including its boundaries and parameters.
/// The start is inclusive and the end is exclusive. In a valid EraHistory, the
/// end of each era will equal the start of the next one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EraSummary {
    /// Start boundary of this era.
    pub start: EraBound,
    /// End boundary of this era (`None` for the current/last era).
    pub end: Option<EraBound>,
    /// Parameters for this era.
    pub params: EraParams,
}

impl EraSummary {
    /// Checks whether the current `EraSummary` ends after the given slot; In case
    /// where the EraSummary doesn't have any upper bound, then we check whether the
    /// point is within a foreseeable horizon.
    pub fn contains_slot(&self, slot: Slot, tip: Slot, stability_window: u64) -> bool {
        self.end
            .as_ref()
            .map(|end| end.slot)
            .unwrap_or_else(|| self.calculate_end_bound(tip, stability_window).slot)
            > slot
    }

    /// Like contains_slot, but doesn't enforce anything about the upper bound. So when there's no
    /// upper bound, the slot is simply always considered within the era.
    pub fn contains_slot_unchecked_horizon(&self, slot: Slot) -> bool {
        self.end.as_ref().map(|end| end.slot > slot).unwrap_or(true)
    }

    pub fn contains_epoch(&self, epoch: u64, tip: Slot, stability_window: u64) -> bool {
        self.end
            .as_ref()
            .map(|end| end.epoch)
            .unwrap_or_else(|| self.calculate_end_bound(tip, stability_window).epoch)
            > epoch
    }

    /// Like contains_epoch, but doesn't enforce anything about the upper bound. So when there's no
    /// upper bound, the epoch is simply always considered within the era.
    pub fn contains_epoch_unchecked_horizon(&self, epoch: u64) -> bool {
        self.end.as_ref().map(|end| end.epoch > epoch).unwrap_or(true)
    }

    /// Calculate a virtual end `EraBound` given a time and the last era summary that we know of.
    ///
    /// **pre-condition**: the provided tip must be after (or equal) to the start of this era.
    fn calculate_end_bound(&self, tip: Slot, stability_window: u64) -> EraBound {
        let Self { start, params, end } = self;
        debug_assert!(end.is_none());

        // NOTE: The +1 here is justified by the fact that upper bound in era summaries are
        // exclusive. So if our tip is *exactly* at the frontier of the stability area, then
        // technically, we already can foresee time in the next epoch.
        let end_of_stable_window = start.slot.max(tip + 1) + stability_window;

        let delta_slots = end_of_stable_window - start.slot;

        let delta_epochs = delta_slots / params.epoch_size_slots
            + if delta_slots.is_multiple_of(params.epoch_size_slots) {
                0
            } else {
                1
            };

        let max_foreseeable_epoch = start.epoch + delta_epochs;

        let foreseeable_slots = delta_epochs * params.epoch_size_slots;

        EraBound {
            time: Duration::from_secs(
                start.time.as_secs() + params.slot_length.as_secs() * foreseeable_slots,
            ),
            slot: start.slot + foreseeable_slots,
            epoch: max_foreseeable_epoch,
        }
    }
}
