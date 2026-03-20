use acropolis_common::protocol_params::ProtocolParams;
use acropolis_common::{DataHash, Era, Lovelace};
use anyhow::{anyhow, Result};
use pallas::codec as pallas_codec;
use pallas::codec::utils::Nullable;
use pallas::ledger::primitives::{alonzo, babbage, conway};
use pallas::ledger::traverse::{MultiEraOutput, MultiEraTx};

fn get_alonzo_value_size_in_bytes(val: &alonzo::Value) -> u64 {
    let mut buf = Vec::new();
    let _ = pallas_codec::minicbor::encode(val, &mut buf);
    buf.len() as u64
}

fn get_alonzo_value_size_in_words(val: &alonzo::Value) -> u64 {
    get_alonzo_value_size_in_bytes(val).div_ceil(8)
}

fn shelley_ma_compute_min_lovelace(
    output: &alonzo::TransactionOutput,
    protocol_params: &ProtocolParams,
) -> Result<Lovelace> {
    let min_utxo_value =
        protocol_params.min_utxo_value().ok_or_else(|| anyhow!("Min UTxO value are not set"))?;
    match &output.amount {
        alonzo::Value::Coin(_) => Ok(min_utxo_value),
        alonzo::Value::Multiasset(lovelace, _) => {
            let utxo_entry_size = 27 + get_alonzo_value_size_in_words(&output.amount);
            let coins_per_utxo_word = min_utxo_value / 27;
            Ok((*lovelace).max(coins_per_utxo_word * utxo_entry_size))
        }
    }
}

fn alonzo_compute_min_lovelace(
    output: &alonzo::TransactionOutput,
    protocol_params: &ProtocolParams,
) -> Result<Lovelace> {
    let lovelace_per_utxo_word = protocol_params
        .lovelace_per_utxo_word()
        .ok_or_else(|| anyhow!("Lovelace per utxo word are not set"))?;
    let output_entry_size: u64 = get_alonzo_value_size_in_words(&output.amount)
        + match output.datum_hash {
            Some(_) => 37, // utxoEntrySizeWithoutVal (27) + dataHashSize (10)
            None => 27,    // utxoEntrySizeWithoutVal
        };
    Ok(lovelace_per_utxo_word * output_entry_size)
}

fn get_babbage_value_size_in_bytes(output: &babbage::MintedTransactionOutput) -> u64 {
    let value = match output {
        babbage::MintedTransactionOutput::Legacy(output) => &output.amount,
        babbage::MintedTransactionOutput::PostAlonzo(output) => &output.value,
    };
    get_alonzo_value_size_in_bytes(value)
}

fn get_babbage_value_size_in_words(output: &babbage::MintedTransactionOutput) -> u64 {
    get_babbage_value_size_in_bytes(output).div_ceil(8)
}

fn babbage_compute_min_lovelace(
    output: &babbage::MintedTransactionOutput,
    protocol_params: &ProtocolParams,
) -> Result<Lovelace> {
    let lovelace_per_utxo_word = protocol_params
        .lovelace_per_utxo_word()
        .ok_or_else(|| anyhow!("Lovelace per utxo word are not set"))?;

    Ok(lovelace_per_utxo_word * (get_babbage_value_size_in_words(output) + 160))
}

fn get_conway_value_size_in_bytes(output: &conway::MintedTransactionOutput) -> u64 {
    match output {
        conway::MintedTransactionOutput::Legacy(output) => {
            get_alonzo_value_size_in_bytes(&output.amount)
        }
        conway::MintedTransactionOutput::PostAlonzo(output) => {
            let mut buf = Vec::new();
            let _ = pallas_codec::minicbor::encode(&output.value, &mut buf);
            buf.len() as u64
        }
    }
}

fn get_conway_value_size_in_words(output: &conway::MintedTransactionOutput) -> u64 {
    get_conway_value_size_in_bytes(output).div_ceil(8)
}

fn conway_compute_min_lovelace(
    output: &conway::MintedTransactionOutput,
    protocol_params: &ProtocolParams,
) -> Result<Lovelace> {
    let lovelace_per_utxo_word = protocol_params
        .lovelace_per_utxo_word()
        .ok_or_else(|| anyhow!("Lovelace per utxo word are not set"))?;
    Ok(lovelace_per_utxo_word * (get_conway_value_size_in_words(output) + 160))
}

pub fn get_value_size_in_bytes(output: &MultiEraOutput) -> u64 {
    match output {
        MultiEraOutput::AlonzoCompatible(output, _) => {
            get_alonzo_value_size_in_bytes(&output.amount)
        }
        MultiEraOutput::Babbage(output) => get_babbage_value_size_in_bytes(output),
        MultiEraOutput::Conway(output) => get_conway_value_size_in_bytes(output),
        _ => 0,
    }
}

#[allow(dead_code)]
pub fn get_value_size_in_words(output: &MultiEraOutput) -> u64 {
    match output {
        MultiEraOutput::AlonzoCompatible(output, _) => {
            get_alonzo_value_size_in_words(&output.amount)
        }
        MultiEraOutput::Babbage(output) => get_babbage_value_size_in_words(output),
        MultiEraOutput::Conway(output) => get_conway_value_size_in_words(output),
        _ => 0,
    }
}

/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/mary/impl/src/Cardano/Ledger/Mary/TxOut.hs#L52
pub fn compute_min_lovelace(
    output: &MultiEraOutput,
    protocol_params: &ProtocolParams,
    era: Era,
) -> Result<Lovelace> {
    match era {
        Era::Byron => Ok(0),
        Era::Shelley | Era::Allegra | Era::Mary => match output {
            MultiEraOutput::AlonzoCompatible(output, _) => Ok(shelley_ma_compute_min_lovelace(
                output.as_ref(),
                protocol_params,
            )?),
            _ => Err(anyhow!("Invalid output for AlonzoCompatible era")),
        },
        Era::Alonzo => match output {
            MultiEraOutput::AlonzoCompatible(output, _) => Ok(alonzo_compute_min_lovelace(
                output.as_ref(),
                protocol_params,
            )?),
            _ => Err(anyhow!("Invalid output for AlonzoCompatible era")),
        },
        Era::Babbage => match output {
            MultiEraOutput::Babbage(output) => Ok(babbage_compute_min_lovelace(
                output.as_ref(),
                protocol_params,
            )?),
            _ => Err(anyhow!("Invalid output for Babbage era")),
        },
        Era::Conway => match output {
            MultiEraOutput::Conway(output) => Ok(conway_compute_min_lovelace(
                output.as_ref(),
                protocol_params,
            )?),
            _ => Err(anyhow!("Invalid output for Conway era")),
        },
    }
}

pub fn get_aux_data_hash(tx: &MultiEraTx) -> Result<Option<DataHash>> {
    let aux_data_hash_bytes = match tx {
        MultiEraTx::AlonzoCompatible(tx, _) => tx.transaction_body.auxiliary_data_hash.as_ref(),
        MultiEraTx::Babbage(tx) => tx.transaction_body.auxiliary_data_hash.as_ref(),
        MultiEraTx::Conway(tx) => tx.transaction_body.auxiliary_data_hash.as_ref(),
        _ => None,
    };
    let aux_data_hash = aux_data_hash_bytes
        .map(|x| DataHash::try_from(x.to_vec()).map_err(|_| anyhow!("Invalid metadata hash")))
        .transpose()?;
    Ok(aux_data_hash)
}

pub fn get_aux_data(tx: &MultiEraTx) -> Option<Vec<u8>> {
    match tx {
        MultiEraTx::AlonzoCompatible(tx, _) => match &tx.auxiliary_data {
            Nullable::Some(x) => Some(x.raw_cbor().to_vec()),
            _ => None,
        },
        MultiEraTx::Babbage(tx) => match &tx.auxiliary_data {
            Nullable::Some(x) => Some(x.raw_cbor().to_vec()),
            _ => None,
        },
        MultiEraTx::Conway(tx) => match &tx.auxiliary_data {
            Nullable::Some(x) => Some(x.raw_cbor().to_vec()),
            _ => None,
        },
        _ => None,
    }
}

pub fn has_mir_certificate(tx: &MultiEraTx) -> bool {
    match tx {
        MultiEraTx::AlonzoCompatible(tx, _) => tx
            .transaction_body
            .certificates
            .as_ref()
            .map(|certs| {
                certs.iter().any(|cert| {
                    matches!(cert, alonzo::Certificate::MoveInstantaneousRewardsCert(_))
                })
            })
            .unwrap_or(false),
        MultiEraTx::Babbage(tx) => tx
            .transaction_body
            .certificates
            .as_ref()
            .map(|certs| {
                certs.iter().any(|cert| {
                    matches!(cert, babbage::Certificate::MoveInstantaneousRewardsCert(_))
                })
            })
            .unwrap_or(false),
        _ => false,
    }
}
