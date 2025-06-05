use anyhow::{Result, *};
use bech32::{Bech32, Hrp};
use std::{fmt::Display, fmt::Formatter, result::Result::Ok};
use crate::types::*;

#[derive(Default)]
struct Encoder {
    data: Vec<u8>
}

impl Encoder {
    pub fn push_u8(&mut self, num: u8) {
        self.data.push(num);
    }

    pub fn push_uvar(&mut self, num: u64) {
        let mut len = 7;
        while (len != 70) && ((num >> len) != 0) {
            len += 7;
        }

        while len > 7 {
            len -= 7;
            self.data.push((num >> len) as u8 | 0x80);
        }
        self.data.push((num & 0x7f) as u8);
    }

    pub fn append(&mut self, v: &Vec<u8>) {
        self.data.extend(v.iter());
    }

    pub fn update(&mut self, idx: usize, updater: impl Fn(u8) -> u8) -> Result<()> {
        match self.data.get_mut(idx) {
            Some(ref mut x) => **x = updater(**x),
            None => return Err(anyhow!("No index {idx} in encoder {:?}", self.data))
        }
        Ok(())
    }

    pub fn to_vec(self) -> Vec<u8> { self.data }
}

fn encode_pointer(e: &mut Encoder, p: &ShelleyAddressPointer) {
    e.push_uvar(p.slot);
    e.push_uvar(p.tx_index);
    e.push_uvar(p.cert_index);
}

fn encode_network(network: &AddressNetwork) -> u8 {
    match network {
        AddressNetwork::Main => 0x1,
        AddressNetwork::Test => 0x0
    }
}

fn encode_shelley_address(address: &ShelleyAddress) -> Result<Vec<u8>> {
    let mut e = Encoder::default();

    let network = encode_network(&address.network);
    match &address.payment {
        ShelleyAddressPaymentPart::PaymentKeyHash(k) => { e.push_u8(network); e.append(k); }
        ShelleyAddressPaymentPart::ScriptHash(s) => { e.push_u8(network | 0x10); e.append(s); }
    }

    let prefix = match &address.delegation {
        ShelleyAddressDelegationPart::StakeKeyHash(k) => { e.append(k); 0x0 },
        ShelleyAddressDelegationPart::ScriptHash(k) => { e.append(k); 0x20 },
        ShelleyAddressDelegationPart::Pointer(p) => { encode_pointer(&mut e, p); 0x40 },
        ShelleyAddressDelegationPart::None => 0x60,
    };

    e.update(0, |x| x | prefix)?;
    Ok(e.to_vec())
}

fn encode_stake_address(address: &StakeAddress) -> Vec<u8> {
    let mut e = Encoder::default();
    let network = encode_network(&address.network);

    match &address.payload {
        StakeAddressPayload::StakeKeyHash(k) => { e.push_u8(network | 0xe0); e.append(k); }
        StakeAddressPayload::ScriptHash(k) => { e.push_u8(network | 0xf0); e.append(k); }
    }

    e.to_vec()
}

fn addr_to_bech32(name: &str, buf: &Vec<u8>) -> String {
    let addr_hrp: Hrp = Hrp::parse(name).unwrap();
    bech32::encode::<Bech32>(addr_hrp, &buf)
        .unwrap_or_else(|e| format!("Cannot convert address to bech32: {e}"))
}

impl StakeAddressPayload {
    fn to_bech32(&self) -> String {
        let mut e = Encoder::default();

        return match &self {
            StakeAddressPayload::StakeKeyHash(k) => { e.append(&k); addr_to_bech32("stake_vkh", &k.to_vec()) }
            StakeAddressPayload::ScriptHash(k) => { e.append(&k); addr_to_bech32("script", &k.to_vec()) }
        }
    }
}

impl Display for StakeAddressPayload {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
         write!(f, "{}", self.to_bech32())
    }
}

impl ShelleyAddress {
    pub fn to_bech32(&self) -> String {
        match &encode_shelley_address(&self) {
            Ok(buf) => addr_to_bech32("addr", buf),
            Err(e) => return format!("Cannot serialize {self:?} to byte string: {e}")
        }
    }
}

impl StakeAddress {
    pub fn to_bech32(&self) -> String {
        addr_to_bech32("stake", &encode_stake_address(self))
    }
}

impl Display for StakeAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_bech32())
    }
}

impl Address {
    pub fn to_bech32(&self) -> String {
        match self {
            Address::None => "none".to_string(),
            Address::Byron(b) => bs58::encode(&b.payload).into_string(),
            Address::Shelley(s) => s.to_bech32(),
            Address::Stake(s) => s.to_bech32()
        }
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_bech32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialize_uint(arg: u64) -> Vec<u8> {
        let mut e = Encoder::default();
        e.push_uvar(arg);
        e.to_vec()
    }

    #[test]
    fn unit_serialization_test() {
        assert_eq!(serialize_uint(0), vec![0]);
        assert_eq!(serialize_uint(1), vec![1]);
        assert_eq!(serialize_uint(0x7f), vec![0x7f]);
        assert_eq!(serialize_uint(0x80), vec![0x81,0]);
        assert_eq!(serialize_uint(0x4000), vec![0x81,0x80,0]);
        assert_eq!(serialize_uint(0x400), vec![0x88,0]);

        for x in 7..63 {
            let val = 1 << x;
            let mut s = Vec::new();
            s.push(0x80 | (1 << (x % 7)));
            for _i in 1..(x / 7) { s.push(0x80); }
            s.push(0);
            assert_eq!(serialize_uint(val), s);
        }
    }
}
