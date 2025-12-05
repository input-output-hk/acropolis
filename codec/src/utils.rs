use acropolis_common::{hash::Hash, rational_number::RationalNumber, *};
use anyhow::{Result, anyhow};
use pallas_primitives::{
    ExUnitPrices as PallasExUnitPrices, Nullable, Relay as PallasRelay, conway,
};
use std::net::{Ipv4Addr, Ipv6Addr};

/// Convert a Pallas Hash reference to an Acropolis Hash (owned)
/// Works for any hash size N
pub fn to_hash<const N: usize>(pallas_hash: &pallas_primitives::Hash<N>) -> Hash<N> {
    Hash::try_from(pallas_hash.as_ref()).unwrap()
}

/// Convert a Pallas Hash reference to an Acropolis Hash (owned)
/// Works for any hash size N
pub fn genesis_to_hash(pallas_hash: &pallas_primitives::Genesishash) -> Hash<28> {
    Hash::try_from(pallas_hash.as_ref()).unwrap()
}

/// Convert a Pallas Hash reference to an Acropolis Hash (owned)
/// Works for any hash size N
pub fn genesis_delegate_to_hash(pallas_hash: &pallas_primitives::GenesisDelegateHash) -> PoolId {
    PoolId::try_from(pallas_hash.as_ref()).unwrap()
}

/// Convert a Pallas Hash<28> reference to an Acropolis PoolId
pub fn to_pool_id(pallas_hash: &pallas_primitives::Hash<28>) -> PoolId {
    to_hash(pallas_hash).into()
}

/// Convert a Pallas Hash<32> reference to an Acropolis VRFKey
pub fn to_vrf_key(pallas_hash: &pallas_primitives::Hash<32>) -> VrfKeyHash {
    VrfKeyHash::try_from(pallas_hash.as_ref()).unwrap()
}

pub fn map_nullable<Src: Clone, Dst>(
    f: impl FnOnce(&Src) -> Dst,
    nullable_src: &Nullable<Src>,
) -> Option<Dst> {
    match nullable_src {
        Nullable::Some(src) => Some(f(src)),
        _ => None,
    }
}

pub fn map_nullable_result<Src: Clone, Dst>(
    f: impl FnOnce(&Src) -> Result<Dst>,
    nullable_src: &Nullable<Src>,
) -> Result<Option<Dst>> {
    match nullable_src {
        Nullable::Some(src) => {
            let res = f(src)?;
            Ok(Some(res))
        }
        _ => Ok(None),
    }
}

pub fn map_unit_interval(pallas_interval: &conway::UnitInterval) -> RationalNumber {
    RationalNumber::new(pallas_interval.numerator, pallas_interval.denominator)
}

pub fn map_ex_units(pallas_units: &conway::ExUnits) -> ExUnits {
    ExUnits {
        mem: pallas_units.mem,
        steps: pallas_units.steps,
    }
}

pub fn map_execution_costs(pallas_ex_costs: &PallasExUnitPrices) -> ExUnitPrices {
    ExUnitPrices {
        mem_price: map_unit_interval(&pallas_ex_costs.mem_price),
        step_price: map_unit_interval(&pallas_ex_costs.step_price),
    }
}

/// Map a Pallas Relay to ours
pub fn map_relay(relay: &PallasRelay) -> Relay {
    match relay {
        PallasRelay::SingleHostAddr(port, ipv4, ipv6) => Relay::SingleHostAddr(SingleHostAddr {
            port: match port {
                Nullable::Some(port) => Some(*port as u16),
                _ => None,
            },
            ipv4: match ipv4 {
                Nullable::Some(ipv4) => <[u8; 4]>::try_from(ipv4).ok().map(Ipv4Addr::from),
                _ => None,
            },
            ipv6: match ipv6 {
                Nullable::Some(ipv6) => <[u8; 16]>::try_from(ipv6).ok().map(Ipv6Addr::from),
                _ => None,
            },
        }),
        PallasRelay::SingleHostName(port, dns_name) => Relay::SingleHostName(SingleHostName {
            port: match port {
                Nullable::Some(port) => Some(*port as u16),
                _ => None,
            },
            dns_name: dns_name.clone(),
        }),
        PallasRelay::MultiHostName(dns_name) => Relay::MultiHostName(MultiHostName {
            dns_name: dns_name.clone(),
        }),
    }
}

/// Map a Pallas DRep to our DRepChoice
pub fn map_drep(drep: &conway::DRep) -> DRepChoice {
    match drep {
        conway::DRep::Key(key_hash) => DRepChoice::Key(to_hash(key_hash)),
        conway::DRep::Script(script_hash) => DRepChoice::Script(to_hash(script_hash)),
        conway::DRep::Abstain => DRepChoice::Abstain,
        conway::DRep::NoConfidence => DRepChoice::NoConfidence,
    }
}

pub fn map_gov_action_id(pallas_action_id: &conway::GovActionId) -> Result<GovActionId> {
    let act_idx_u8: u8 = match pallas_action_id.action_index.try_into() {
        Ok(v) => v,
        Err(e) => return Err(anyhow!("Invalid action index {e}")),
    };

    Ok(GovActionId {
        transaction_id: TxHash::from(*pallas_action_id.transaction_id),
        action_index: act_idx_u8,
    })
}

pub fn map_nullable_gov_action_id(
    id: &Nullable<conway::GovActionId>,
) -> Result<Option<GovActionId>> {
    map_nullable_result(map_gov_action_id, id)
}

pub fn map_anchor(anchor: &conway::Anchor) -> Anchor {
    Anchor {
        url: anchor.url.clone(),
        data_hash: anchor.content_hash.to_vec(),
    }
}

/// Map a Nullable Anchor to ours
pub fn map_nullable_anchor(anchor: &Nullable<conway::Anchor>) -> Option<Anchor> {
    map_nullable(map_anchor, anchor)
}
