use crate::{
    MultiHostName, PoolId, PoolRegistration, Ratio, Relay, SingleHostAddr, SingleHostName,
};
use anyhow::{bail, Context, Result};
use minicbor::data::Tag;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct LedgerState {
    pub spo_state: SPOState,
}

pub struct UTxOState {}

pub struct StakeDistributionState {}

pub struct AccountState {}

pub struct ParametersState {}

#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Decode,
    minicbor::Encode,
    Default,
    Eq,
    PartialEq,
)]
pub struct SPOState {
    #[n(0)]
    pub pools: BTreeMap<PoolId, PoolRegistration>,
    #[n(1)]
    pub updates: BTreeMap<PoolId, PoolRegistration>,
    #[n(2)]
    pub retiring: BTreeMap<PoolId, u64>,
}

pub struct DRepState {}

pub struct ProposalState {}

pub struct VotingState {}

impl LedgerState {
    pub fn from_directory(directory_path: impl AsRef<Path>) -> Result<Self> {
        let directory_path = directory_path.as_ref();
        if !directory_path.exists() {
            bail!("directory does not exist: {}", directory_path.display());
        }

        if !directory_path.is_dir() {
            bail!("path is not a directory: {}", directory_path.display());
        }

        let mut ledger_state = Self::default();
        ledger_state.load_from_directory(directory_path).with_context(|| {
            format!(
                "Failed to load ledger state from directory: {}",
                directory_path.display()
            )
        })?;

        Ok(ledger_state)
    }

    fn load_from_directory(&mut self, directory_path: impl AsRef<Path>) -> Result<()> {
        let directory_path = directory_path.as_ref();
        let entries = fs::read_dir(directory_path)
            .with_context(|| format!("failed to read directory: {}", directory_path.display()))?;

        for entry in entries {
            let entry = entry.with_context(|| "failed to read directory entry")?;
            let path = entry.path();

            if path.is_file() && path.extension().is_some_and(|ext| ext == "cbor") {
                self.load_cbor_file(&path)
                    .with_context(|| format!("failed to load CBOR file: {}", path.display()))?;
            }
        }

        Ok(())
    }

    fn load_cbor_file(&mut self, file_path: impl AsRef<Path>) -> Result<()> {
        let file_path = file_path.as_ref();
        let filename = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .with_context(|| format!("invalid filename: {}", file_path.display()))?;

        let bytes = fs::read(file_path)
            .with_context(|| format!("failed to read file: {}", file_path.display()))?;

        match filename {
            "pools" => {
                self.spo_state = minicbor::decode(&bytes).with_context(|| {
                    format!("failed to decode SPO state from: {}", file_path.display())
                })?;
            }
            _ => {
                // ignore unknown cbor files
            }
        }

        Ok(())
    }
}

impl<'b, C> minicbor::decode::Decode<'b, C> for Ratio {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let tag = d.tag()?;
        if tag.as_u64() != 30 {
            return Err(minicbor::decode::Error::message("tag must be 30"));
        }
        let maybe_array_length = d.array()?;
        if let Some(length) = maybe_array_length {
            if length != 2 {
                return Err(minicbor::decode::Error::message(
                    "array must be of length 2",
                ));
            }
        }

        Ok(Ratio {
            numerator: d.decode_with(ctx)?,
            denominator: d.decode_with(ctx)?,
        })
    }
}

impl<C> minicbor::encode::Encode<C> for Ratio {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.tag(Tag::new(30))?;
        e.array(2)?;
        e.encode_with(self.numerator, ctx)?;
        e.encode_with(self.denominator, ctx)?;
        Ok(())
    }
}

impl<'b, C> minicbor::decode::Decode<'b, C> for Relay {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let variant = d.u16()?;

        match variant {
            0 => Ok(Relay::SingleHostAddr(SingleHostAddr {
                port: d.decode_with(ctx)?,
                ipv4: d.decode_with(ctx)?,
                ipv6: d.decode_with(ctx)?,
            })),
            1 => Ok(Relay::SingleHostName(SingleHostName {
                port: d.decode_with(ctx)?,
                dns_name: d.decode_with(ctx)?,
            })),
            2 => Ok(Relay::MultiHostName(MultiHostName {
                dns_name: d.decode_with(ctx)?,
            })),
            _ => Err(minicbor::decode::Error::message(
                "invalid variant id for Relay",
            )),
        }
    }
}

impl<C> minicbor::encode::Encode<C> for Relay {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            Relay::SingleHostAddr(SingleHostAddr { port, ipv4, ipv6 }) => {
                e.array(4)?;
                e.encode_with(0, ctx)?;
                e.encode_with(port, ctx)?;
                e.encode_with(ipv4, ctx)?;
                e.encode_with(ipv6, ctx)?;

                Ok(())
            }
            Relay::SingleHostName(SingleHostName { port, dns_name }) => {
                e.array(3)?;
                e.encode_with(1, ctx)?;
                e.encode_with(port, ctx)?;
                e.encode_with(dns_name, ctx)?;

                Ok(())
            }
            Relay::MultiHostName(MultiHostName { dns_name }) => {
                e.array(2)?;
                e.encode_with(2, ctx)?;
                e.encode_with(dns_name, ctx)?;

                Ok(())
            }
        }
    }
}
