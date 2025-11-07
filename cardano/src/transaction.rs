use acropolis_common::{Lovelace, protocol_params::ProtocolParams};
use anyhow::{Error, anyhow};

pub fn calculate_transaction_fee(
    recorded_fee: &Option<Lovelace>,
    inputs: &[Lovelace],
    outputs: &[Lovelace],
) -> Lovelace {
    match recorded_fee {
        Some(fee) => *fee,
        None => inputs.iter().sum::<Lovelace>() - outputs.iter().sum::<Lovelace>(),
    }
}

pub fn calculate_deposit(
    pool_update_count: u64,
    stake_cert_count: u64,
    params: &ProtocolParams,
) -> Result<Lovelace, Error> {
    match &params.shelley {
        Some(shelley) => Ok(stake_cert_count * shelley.protocol_params.key_deposit
            + pool_update_count * shelley.protocol_params.pool_deposit),
        None => {
            if pool_update_count > 0 || stake_cert_count > 0 {
                Err(anyhow!("No Shelley params, but deposits present"))
            } else {
                Ok(0)
            }
        }
    }
}
