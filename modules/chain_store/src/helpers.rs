use std::{collections::BTreeMap, sync::Arc};

use acropolis_common::{
    crypto::keyhash_224,
    queries::{
        blocks::{
            BlockInfo, BlockInvolvedAddress, BlockInvolvedAddresses, BlockKey, BlockTransaction,
            BlockTransactions, BlockTransactionsCBOR, CompactBlockInfo,
        },
        misc::Order,
        transactions::{
            TransactionDelegationCertificate, TransactionInfo, TransactionMIR,
            TransactionMetadataItem, TransactionOutputAmount, TransactionPoolRetirementCertificate,
            TransactionPoolUpdateCertificate, TransactionStakeCertificate, TransactionWithdrawal,
        },
    },
    AssetName, BechOrdAddress, BlockHash, InstantaneousRewardSource, NativeAsset, NetworkId,
    StakeAddress, TxHash,
};
use anyhow::{anyhow, Result};
use pallas::ledger::primitives::{alonzo, conway};
use pallas_traverse::{MultiEraCert, MultiEraMeta};

use crate::{
    state::State,
    stores::{Block, Store, Tx},
};

pub fn get_block_by_key(store: &Arc<dyn Store>, block_key: &BlockKey) -> Result<Option<Block>> {
    match block_key {
        BlockKey::Hash(hash) => store.get_block_by_hash(hash.as_ref()),
        BlockKey::Number(number) => store.get_block_by_number(*number),
    }
}

pub fn get_block_number(block: &Block) -> Result<u64> {
    Ok(pallas_traverse::MultiEraBlock::decode(&block.bytes)?.number())
}

pub fn get_block_hash(block: &Block) -> Result<BlockHash> {
    Ok(BlockHash::from(
        *pallas_traverse::MultiEraBlock::decode(&block.bytes)?.hash(),
    ))
}

pub fn to_block_info(
    block: Block,
    store: &Arc<dyn Store>,
    state: &State,
    is_latest: bool,
) -> Result<BlockInfo> {
    let blocks = vec![block];
    let mut info = to_block_info_bulk(blocks, store, state, is_latest)?;
    Ok(info.remove(0))
}

pub fn to_compact_block_info(block: Block) -> Result<CompactBlockInfo> {
    let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
    let header = decoded.header();

    Ok(CompactBlockInfo {
        timestamp: block.extra.timestamp,
        number: header.number(),
        hash: BlockHash::from(*header.hash()),
        slot: header.slot(),
        epoch: block.extra.epoch,
    })
}

pub fn to_block_info_bulk(
    blocks: Vec<Block>,
    store: &Arc<dyn Store>,
    state: &State,
    final_block_is_latest: bool,
) -> Result<Vec<BlockInfo>> {
    if blocks.is_empty() {
        return Ok(vec![]);
    }
    let mut decoded_blocks = vec![];
    for block in &blocks {
        decoded_blocks.push(pallas_traverse::MultiEraBlock::decode(&block.bytes)?);
    }

    let (latest_number, latest_hash) = if final_block_is_latest {
        let latest = decoded_blocks.last().unwrap();
        (latest.number(), latest.hash())
    } else {
        let raw_latest = store.get_latest_block()?.unwrap();
        let latest = pallas_traverse::MultiEraBlock::decode(&raw_latest.bytes)?;
        (latest.number(), latest.hash())
    };

    let mut next_hash = if final_block_is_latest {
        None
    } else {
        let next_number = decoded_blocks.last().unwrap().number() + 1;
        if next_number > latest_number {
            None
        } else if next_number == latest_number {
            Some(latest_hash)
        } else {
            let raw_next = store.get_block_by_number(next_number)?;
            if let Some(raw_next) = raw_next {
                let next = pallas_traverse::MultiEraBlock::decode(&raw_next.bytes)?;
                Some(next.hash())
            } else {
                None
            }
        }
    };

    let mut block_info = vec![];
    for (block, decoded) in blocks.iter().zip(decoded_blocks).rev() {
        let header = decoded.header();
        let mut output = None;
        let mut fees = None;
        for tx in decoded.txs() {
            if let Some(new_fee) = tx.fee() {
                fees = Some(fees.unwrap_or_default() + new_fee);
            }
            for o in tx.outputs() {
                output = Some(output.unwrap_or_default() + o.value().coin())
            }
        }
        let (op_cert_hot_vkey, op_cert_counter) = match &header {
            pallas_traverse::MultiEraHeader::BabbageCompatible(h) => {
                let cert = &h.header_body.operational_cert;
                (
                    Some(&cert.operational_cert_hot_vkey),
                    Some(cert.operational_cert_sequence_number),
                )
            }
            pallas_traverse::MultiEraHeader::ShelleyCompatible(h) => (
                Some(&h.header_body.operational_cert_hot_vkey),
                Some(h.header_body.operational_cert_sequence_number),
            ),
            _ => (None, None),
        };
        let op_cert = op_cert_hot_vkey.map(|vkey| keyhash_224(vkey));

        block_info.push(BlockInfo {
            timestamp: block.extra.timestamp,
            number: header.number(),
            hash: BlockHash::from(*header.hash()),
            slot: header.slot(),
            epoch: block.extra.epoch,
            epoch_slot: block.extra.epoch_slot,
            issuer: acropolis_codec::map_to_block_issuer(
                &header,
                &state.byron_heavy_delegates.clone().into_iter().collect(),
                &state.shelley_genesis_delegates,
            ),
            size: block.bytes.len() as u64,
            tx_count: decoded.tx_count() as u64,
            output,
            fees,
            block_vrf: header.vrf_vkey().map(|key| key.try_into().ok().unwrap()),
            op_cert,
            op_cert_counter,
            previous_block: header.previous_hash().map(|h| BlockHash::from(*h)),
            next_block: next_hash.map(|h| BlockHash::from(*h)),
            confirmations: latest_number - header.number(),
        });

        next_hash = Some(header.hash());
    }

    block_info.reverse();
    Ok(block_info)
}

pub fn to_block_transaction_hashes(block: &Block) -> Result<Vec<TxHash>> {
    let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
    let txs = decoded.txs();
    Ok(txs.iter().map(|tx| TxHash::from(*tx.hash())).collect())
}

pub fn to_block_transactions(
    block: Block,
    limit: &u64,
    skip: &u64,
    order: &Order,
) -> Result<BlockTransactions> {
    let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
    let txs = decoded.txs();
    let txs_iter: Box<dyn Iterator<Item = _>> = match *order {
        Order::Asc => Box::new(txs.iter()),
        Order::Desc => Box::new(txs.iter().rev()),
    };
    let hashes = txs_iter
        .skip(*skip as usize)
        .take(*limit as usize)
        .map(|tx| TxHash::from(*tx.hash()))
        .collect();
    Ok(BlockTransactions { hashes })
}

pub fn to_block_transactions_cbor(
    block: Block,
    limit: &u64,
    skip: &u64,
    order: &Order,
) -> Result<BlockTransactionsCBOR> {
    let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
    let txs = decoded.txs();
    let txs_iter: Box<dyn Iterator<Item = _>> = match *order {
        Order::Asc => Box::new(txs.iter()),
        Order::Desc => Box::new(txs.iter().rev()),
    };
    let txs = txs_iter
        .skip(*skip as usize)
        .take(*limit as usize)
        .map(|tx| {
            let hash = TxHash::from(*tx.hash());
            let cbor = tx.encode();
            BlockTransaction { hash, cbor }
        })
        .collect();
    Ok(BlockTransactionsCBOR { txs })
}

pub fn to_block_involved_addresses(
    block: Block,
    limit: &u64,
    skip: &u64,
) -> Result<BlockInvolvedAddresses> {
    let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
    let mut addresses = BTreeMap::new();
    for tx in decoded.txs() {
        let hash = TxHash::from(*tx.hash());
        for output in tx.outputs() {
            if let Ok(pallas_address) = output.address() {
                if let Ok(address) = acropolis_codec::map_address(&pallas_address) {
                    addresses.entry(BechOrdAddress(address)).or_insert_with(Vec::new).push(hash);
                }
            }
        }
    }
    let addresses: Vec<BlockInvolvedAddress> = addresses
        .into_iter()
        .skip(*skip as usize)
        .take(*limit as usize)
        .map(|(address, txs)| BlockInvolvedAddress {
            address: address.0,
            txs,
        })
        .collect();
    Ok(BlockInvolvedAddresses { addresses })
}

pub fn to_tx_info(tx: &Tx) -> Result<TransactionInfo> {
    let block = pallas_traverse::MultiEraBlock::decode(&tx.block.bytes)?;
    let txs = block.txs();
    let Some(tx_decoded) = txs.get(tx.index as usize) else {
        return Err(anyhow!("Transaction not found in block for given index"));
    };
    let mut output_amounts = Vec::new();
    for output in tx_decoded.outputs() {
        let value = output.value();
        let lovelace_amount = value.coin();
        if lovelace_amount != 0 {
            output_amounts.push(TransactionOutputAmount::Lovelace(lovelace_amount));
        }
        for policy in value.assets() {
            for asset in policy.assets() {
                if asset.is_output() {
                    output_amounts.push(TransactionOutputAmount::Asset(NativeAsset {
                        name: AssetName::new(asset.name()).ok_or(anyhow!("Bad asset name"))?,
                        amount: asset.output_coin().ok_or(anyhow!("No output amount"))?,
                    }));
                }
            }
        }
    }
    let mut mir_cert_count = 0;
    let mut delegation_count = 0;
    let mut stake_cert_count = 0;
    let mut pool_update_count = 0;
    let mut pool_retire_count = 0;
    // TODO: check counts use all correct certs
    for cert in tx_decoded.certs() {
        match cert {
            MultiEraCert::AlonzoCompatible(cert) => match cert.as_ref().as_ref() {
                alonzo::Certificate::StakeRegistration { .. } => {
                    stake_cert_count += 1;
                }
                alonzo::Certificate::StakeDeregistration { .. } => {
                    stake_cert_count += 1;
                }
                alonzo::Certificate::StakeDelegation { .. } => delegation_count += 1,
                alonzo::Certificate::PoolRegistration { .. } => {
                    pool_update_count += 1;
                }
                alonzo::Certificate::PoolRetirement { .. } => pool_retire_count += 1,
                alonzo::Certificate::MoveInstantaneousRewardsCert { .. } => mir_cert_count += 1,
                _ => (),
            },
            MultiEraCert::Conway(cert) => match cert.as_ref().as_ref() {
                conway::Certificate::StakeRegistration { .. } => {
                    stake_cert_count += 1;
                }
                conway::Certificate::StakeDeregistration { .. } => {
                    stake_cert_count += 1;
                }
                conway::Certificate::StakeDelegation { .. } => delegation_count += 1,
                conway::Certificate::PoolRegistration { .. } => {
                    pool_update_count += 1;
                }
                conway::Certificate::PoolRetirement { .. } => pool_retire_count += 1,
                conway::Certificate::Reg { .. } => stake_cert_count += 1,
                conway::Certificate::UnReg { .. } => stake_cert_count += 1,
                conway::Certificate::StakeRegDeleg { .. } => delegation_count += 1,
                conway::Certificate::VoteRegDeleg { .. } => delegation_count += 1,
                conway::Certificate::StakeVoteRegDeleg { .. } => delegation_count += 1,
                _ => (),
            },
            _ => (),
        }
    }
    Ok(TransactionInfo {
        hash: TxHash::from(*tx_decoded.hash()),
        block_hash: BlockHash::from(*block.hash()),
        block_number: block.number(),
        block_time: tx.block.extra.timestamp,
        epoch: tx.block.extra.epoch,
        slot: block.slot(),
        index: tx.index,
        output_amounts,
        recorded_fee: tx_decoded.fee(),
        // TODO reporting too many bytes (140)
        size: tx_decoded.size() as u64,
        invalid_before: tx_decoded.validity_start(),
        // TODO
        invalid_after: None,
        utxo_count: (tx_decoded.requires().len() + tx_decoded.produces().len()) as u64,
        withdrawal_count: tx_decoded.withdrawals_sorted_set().len() as u64,
        mir_cert_count,
        delegation_count,
        stake_cert_count,
        pool_update_count,
        pool_retire_count,
        asset_mint_or_burn_count: tx_decoded.mints().iter().map(|p| p.assets().len()).sum::<usize>()
            as u64,
        redeemer_count: tx_decoded.redeemers().len() as u64,
        valid_contract: tx_decoded.is_valid(),
    })
}

pub fn to_tx_stakes(tx: &Tx, network_id: NetworkId) -> Result<Vec<TransactionStakeCertificate>> {
    let block = pallas_traverse::MultiEraBlock::decode(&tx.block.bytes)?;
    let txs = block.txs();
    let Some(tx_decoded) = txs.get(tx.index as usize) else {
        return Err(anyhow!("Transaction not found in block for given index"));
    };
    let mut certs = Vec::new();
    // TODO: check cert types
    for (index, cert) in tx_decoded.certs().iter().enumerate() {
        match cert {
            MultiEraCert::AlonzoCompatible(cert) => match cert.as_ref().as_ref() {
                alonzo::Certificate::StakeRegistration(cred) => {
                    certs.push(TransactionStakeCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        registration: true,
                    });
                }
                alonzo::Certificate::StakeDeregistration(cred) => {
                    certs.push(TransactionStakeCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        registration: false,
                    });
                }
                _ => (),
            },
            MultiEraCert::Conway(cert) => match cert.as_ref().as_ref() {
                conway::Certificate::StakeRegistration(cred) => {
                    certs.push(TransactionStakeCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        registration: true,
                    });
                }
                conway::Certificate::StakeDeregistration(cred) => {
                    certs.push(TransactionStakeCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        registration: false,
                    });
                }
                conway::Certificate::StakeRegDeleg(cred, _, _) => {
                    certs.push(TransactionStakeCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        registration: true,
                    });
                }
                conway::Certificate::StakeVoteRegDeleg(cred, _, _, _) => {
                    certs.push(TransactionStakeCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        registration: true,
                    });
                }
                _ => (),
            },
            _ => (),
        }
    }
    Ok(certs)
}

pub fn to_tx_delegations(
    tx: &Tx,
    network_id: NetworkId,
) -> Result<Vec<TransactionDelegationCertificate>> {
    let block = pallas_traverse::MultiEraBlock::decode(&tx.block.bytes)?;
    let txs = block.txs();
    let Some(tx_decoded) = txs.get(tx.index as usize) else {
        return Err(anyhow!("Transaction not found in block for given index"));
    };
    let mut certs = Vec::new();
    for (index, cert) in tx_decoded.certs().iter().enumerate() {
        match cert {
            MultiEraCert::AlonzoCompatible(cert) => {
                if let alonzo::Certificate::StakeDelegation(cred, pool_key_hash) =
                    cert.as_ref().as_ref()
                {
                    certs.push(TransactionDelegationCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        pool_id: acropolis_codec::to_pool_id(pool_key_hash),
                        active_epoch: tx.block.extra.epoch + 1,
                    });
                }
            }
            MultiEraCert::Conway(cert) => match cert.as_ref().as_ref() {
                conway::Certificate::StakeDelegation(cred, pool_key_hash) => {
                    certs.push(TransactionDelegationCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        pool_id: acropolis_codec::to_pool_id(pool_key_hash),
                        active_epoch: tx.block.extra.epoch + 1,
                    });
                }
                conway::Certificate::StakeRegDeleg(cred, pool_key_hash, _) => {
                    certs.push(TransactionDelegationCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        pool_id: acropolis_codec::to_pool_id(pool_key_hash),
                        active_epoch: tx.block.extra.epoch + 1,
                    });
                }
                conway::Certificate::StakeVoteRegDeleg(cred, pool_key_hash, _, _) => {
                    certs.push(TransactionDelegationCertificate {
                        index: index as u64,
                        address: acropolis_codec::map_stake_address(cred, network_id.clone()),
                        pool_id: acropolis_codec::to_pool_id(pool_key_hash),
                        active_epoch: tx.block.extra.epoch + 1,
                    });
                }
                _ => (),
            },
            _ => (),
        }
    }
    Ok(certs)
}

pub fn to_tx_withdrawals(tx: &Tx) -> Result<Vec<TransactionWithdrawal>> {
    let block = pallas_traverse::MultiEraBlock::decode(&tx.block.bytes)?;
    let txs = block.txs();
    let Some(tx_decoded) = txs.get(tx.index as usize) else {
        return Err(anyhow!("Transaction not found in block for given index"));
    };
    let mut withdrawals = Vec::new();
    for (address, amount) in tx_decoded.withdrawals_sorted_set() {
        withdrawals.push(TransactionWithdrawal {
            address: StakeAddress::from_binary(address)?,
            amount,
        });
    }
    Ok(withdrawals)
}

pub fn to_tx_mirs(tx: &Tx, network_id: NetworkId) -> Result<Vec<TransactionMIR>> {
    let block = pallas_traverse::MultiEraBlock::decode(&tx.block.bytes)?;
    let txs = block.txs();
    let Some(tx_decoded) = txs.get(tx.index as usize) else {
        return Err(anyhow!("Transaction not found in block for given index"));
    };
    let mut certs = Vec::new();
    for (cert_index, cert) in tx_decoded.certs().iter().enumerate() {
        if let MultiEraCert::AlonzoCompatible(cert) = cert {
            if let alonzo::Certificate::MoveInstantaneousRewardsCert(cert) = cert.as_ref().as_ref()
            {
                match &cert.target {
                    alonzo::InstantaneousRewardTarget::StakeCredentials(creds) => {
                        for (cred, amount) in creds.clone().to_vec() {
                            certs.push(TransactionMIR {
                                cert_index: cert_index as u64,
                                pot: match cert.source {
                                    alonzo::InstantaneousRewardSource::Reserves => {
                                        InstantaneousRewardSource::Reserves
                                    }
                                    alonzo::InstantaneousRewardSource::Treasury => {
                                        InstantaneousRewardSource::Treasury
                                    }
                                },
                                address: acropolis_codec::map_stake_address(
                                    &cred,
                                    network_id.clone(),
                                ),
                                amount: amount as u64,
                            });
                        }
                    }
                    alonzo::InstantaneousRewardTarget::OtherAccountingPot(_coin) => {
                        // TODO
                    }
                }
            }
        }
    }
    Ok(certs)
}

pub fn to_tx_pool_updates(
    tx: &Tx,
    network_id: NetworkId,
) -> Result<Vec<TransactionPoolUpdateCertificate>> {
    let block = pallas_traverse::MultiEraBlock::decode(&tx.block.bytes)?;
    let txs = block.txs();
    let Some(tx_decoded) = txs.get(tx.index as usize) else {
        return Err(anyhow!("Transaction not found in block for given index"));
    };
    let mut certs = Vec::new();
    for (cert_index, cert) in tx_decoded.certs().iter().enumerate() {
        match cert {
            MultiEraCert::AlonzoCompatible(cert) => {
                if let alonzo::Certificate::PoolRegistration {
                    operator,
                    vrf_keyhash,
                    pledge,
                    cost,
                    margin,
                    reward_account,
                    pool_owners,
                    relays,
                    pool_metadata,
                } = cert.as_ref().as_ref()
                {
                    certs.push(TransactionPoolUpdateCertificate {
                        cert_index: cert_index as u64,
                        pool_reg: acropolis_codec::to_pool_reg(
                            operator,
                            vrf_keyhash,
                            pledge,
                            cost,
                            margin,
                            reward_account,
                            pool_owners,
                            relays,
                            pool_metadata,
                            network_id.clone(),
                            false,
                        )?,
                        // Pool registration/updates become active after 2 epochs
                        active_epoch: tx.block.extra.epoch + 2,
                    });
                }
            }
            MultiEraCert::Conway(cert) => {
                if let conway::Certificate::PoolRegistration {
                    operator,
                    vrf_keyhash,
                    pledge,
                    cost,
                    margin,
                    reward_account,
                    pool_owners,
                    relays,
                    pool_metadata,
                } = cert.as_ref().as_ref()
                {
                    certs.push(TransactionPoolUpdateCertificate {
                        cert_index: cert_index as u64,
                        pool_reg: acropolis_codec::to_pool_reg(
                            operator,
                            vrf_keyhash,
                            pledge,
                            cost,
                            margin,
                            reward_account,
                            pool_owners,
                            relays,
                            pool_metadata,
                            network_id.clone(),
                            false,
                        )?,
                        // Pool registration/updates become active after 2 epochs
                        active_epoch: tx.block.extra.epoch + 2,
                    });
                }
            }
            _ => (),
        }
    }
    Ok(certs)
}

pub fn to_tx_pool_retirements(tx: &Tx) -> Result<Vec<TransactionPoolRetirementCertificate>> {
    let block = pallas_traverse::MultiEraBlock::decode(&tx.block.bytes)?;
    let txs = block.txs();
    let Some(tx_decoded) = txs.get(tx.index as usize) else {
        return Err(anyhow!("Transaction not found in block for given index"));
    };
    let mut certs = Vec::new();
    for (cert_index, cert) in tx_decoded.certs().iter().enumerate() {
        match cert {
            MultiEraCert::AlonzoCompatible(cert) => {
                if let alonzo::Certificate::PoolRetirement(operator, epoch) = cert.as_ref().as_ref()
                {
                    certs.push(TransactionPoolRetirementCertificate {
                        cert_index: cert_index as u64,
                        pool_id: acropolis_codec::to_pool_id(operator),
                        retirement_epoch: *epoch,
                    });
                }
            }
            MultiEraCert::Conway(cert) => {
                if let conway::Certificate::PoolRetirement(operator, epoch) = cert.as_ref().as_ref()
                {
                    certs.push(TransactionPoolRetirementCertificate {
                        cert_index: cert_index as u64,
                        pool_id: acropolis_codec::to_pool_id(operator),
                        retirement_epoch: *epoch,
                    });
                }
            }
            _ => (),
        }
    }
    Ok(certs)
}

pub fn to_tx_metadata(tx: &Tx) -> Result<Vec<TransactionMetadataItem>> {
    let block = pallas_traverse::MultiEraBlock::decode(&tx.block.bytes)?;
    let txs = block.txs();
    let Some(tx_decoded) = txs.get(tx.index as usize) else {
        return Err(anyhow!("Transaction not found in block for given index"));
    };
    let mut items = Vec::new();
    if let MultiEraMeta::AlonzoCompatible(metadata) = tx_decoded.metadata() {
        for (label, datum) in &metadata.clone().to_vec() {
            items.push(TransactionMetadataItem {
                label: label.to_string(),
                json_metadata: acropolis_codec::map_metadatum(datum),
            });
        }
    }
    Ok(items)
}
