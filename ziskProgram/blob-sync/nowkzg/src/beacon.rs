//! Minimal beacon-node HTTP client.
//!
//! Mirrors the slice of functionality op-succinct hands off to `kona-host`
//! (see `utils/host/src/fetcher.rs` `l1_beacon_rpc` plumbing): bootstrap once
//! to learn `genesis_time` + `SECONDS_PER_SLOT`, then resolve a slot from an
//! L1 block timestamp and fetch its blob sidecars.

use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug)]
pub struct BeaconClient {
    http: Client,
    base: Url,
    pub genesis_time: u64,
    pub seconds_per_slot: u64,
}

#[derive(Debug, Deserialize)]
struct GenesisResponse {
    data: GenesisData,
}

#[derive(Debug, Deserialize)]
struct GenesisData {
    #[serde(deserialize_with = "de_str_u64")]
    genesis_time: u64,
}

#[derive(Debug, Deserialize)]
struct SpecResponse {
    data: SpecData,
}

#[derive(Debug, Deserialize)]
struct SpecData {
    #[serde(rename = "SECONDS_PER_SLOT", deserialize_with = "de_str_u64")]
    seconds_per_slot: u64,
}

#[derive(Debug, Deserialize)]
struct SidecarsResponse {
    data: Vec<RawSidecar>,
}

/// Subset of fields we actually need from `/eth/v1/beacon/blob_sidecars/{slot}`.
/// The full schema (see `alloy_rpc_types_beacon::sidecar::BlobData`) also
/// carries the inclusion proof and signed block header — we don't need them
/// because we accept the beacon node as trusted and only sanity-check the
/// commitment ↔ versioned-hash relationship downstream.
#[derive(Debug, Deserialize)]
struct RawSidecar {
    #[serde(deserialize_with = "de_str_u64")]
    index: u64,
    blob: String,            // 0x-prefixed hex, 131072 bytes raw
    kzg_commitment: String,  // 0x-prefixed hex, 48 bytes
    kzg_proof: String,       // 0x-prefixed hex, 48 bytes
}

#[derive(Debug, Clone)]
pub struct Sidecar {
    pub index: u64,
    pub blob: Box<[u8; 131_072]>,
    pub kzg_commitment: [u8; 48],
    pub kzg_proof: [u8; 48],
}

impl BeaconClient {
    pub async fn new(mut base: Url) -> Result<Self> {
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let http = Client::builder()
            .user_agent("blob-sync/0.1")
            .build()
            .context("building reqwest client")?;

        let genesis: GenesisResponse = http
            .get(base.join("eth/v1/beacon/genesis")?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("decoding /eth/v1/beacon/genesis")?;

        let spec: SpecResponse = http
            .get(base.join("eth/v1/config/spec")?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("decoding /eth/v1/config/spec")?;

        Ok(Self {
            http,
            base,
            genesis_time: genesis.data.genesis_time,
            seconds_per_slot: spec.data.seconds_per_slot,
        })
    }

    /// Map an EL block timestamp to the beacon slot whose block carries that
    /// EL payload. After the merge this is exact for every slot containing a
    /// block; missed slots simply have no payload at that timestamp.
    pub fn slot_for_timestamp(&self, timestamp: u64) -> Result<u64> {
        if timestamp < self.genesis_time {
            bail!(
                "block timestamp {timestamp} pre-dates beacon genesis {}",
                self.genesis_time
            );
        }
        Ok((timestamp - self.genesis_time) / self.seconds_per_slot)
    }

    /// Fetch all blob sidecars for the given slot. A 404 (missed slot or pre-
    /// Cancun slot) yields an empty vec rather than an error.
    pub async fn get_blob_sidecars(&self, slot: u64) -> Result<Vec<Sidecar>> {
        let url = self
            .base
            .join(&format!("eth/v1/beacon/blob_sidecars/{slot}"))?;
        let resp = self.http.get(url).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(vec![]);
        }
        let resp = resp.error_for_status()?;
        let parsed: SidecarsResponse = resp
            .json()
            .await
            .with_context(|| format!("decoding blob_sidecars for slot {slot}"))?;

        parsed
            .data
            .into_iter()
            .map(|raw| {
                Ok(Sidecar {
                    index: raw.index,
                    blob: decode_hex_array::<131_072>(&raw.blob, "blob")?,
                    kzg_commitment: *decode_hex_array::<48>(&raw.kzg_commitment, "kzg_commitment")?,
                    kzg_proof: *decode_hex_array::<48>(&raw.kzg_proof, "kzg_proof")?,
                })
            })
            .collect()
    }
}

fn decode_hex_array<const N: usize>(s: &str, field: &str) -> Result<Box<[u8; N]>> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(stripped).with_context(|| format!("hex-decoding {field}"))?;
    if bytes.len() != N {
        return Err(anyhow!(
            "{field} has {} bytes, expected {N}",
            bytes.len()
        ));
    }
    let mut out = Box::new([0u8; N]);
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn de_str_u64<'de, D>(d: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = <String as serde::Deserialize>::deserialize(d)?;
    s.parse().map_err(serde::de::Error::custom)
}
