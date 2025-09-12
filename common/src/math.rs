use anyhow::{bail, Result};

/// Update an unsigned value with a signed delta, with fences
pub fn update_value_with_delta(value: &mut u64, delta: i64) -> Result<()> {
    if delta >= 0 {
        *value = (*value).saturating_add(delta as u64);
    } else {
        let abs = (-delta) as u64;
        if abs > *value {
            bail!("Value underflow - was {}, delta {}", *value, delta);
        } else {
            *value -= abs;
        }
    }

    Ok(())
}
