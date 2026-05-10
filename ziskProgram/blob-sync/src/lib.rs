//! EIP-4844 blob fetching for the OpenBlob host.
//!
//! Walks an L1 block range, pulls every blob via the beacon node, and verifies
//! each sidecar's commitment matches the requested versioned hash. Returns the
//! raw blob bytes in the exact layout `common::GuestInputs::blob_data` expects
//! (concatenated, `BLOB_RAW_BYTES` per blob) so the host can hand them straight
//! to the ZisK guest as a private input.
//!
//! This crate is a **host-only** module: it reaches out to L1 EL/CL endpoints
//! over HTTP, so it cannot be linked into the guest. Decoding helpers in
//! [`decode`] are pure compute and could be reused inside the guest if needed.

pub mod beacon;
pub mod decode;
pub mod kzg;
pub mod l1;

use alloy_eips::eip4844::kzg_to_versioned_hash;
use anyhow::{Context, Result, anyhow};
use common::{BLOB_RAW_BYTES, BlobRefIn};
use std::collections::BTreeMap;
use url::Url;

pub use beacon::{BeaconClient, Sidecar};
pub use decode::{BLOB_BYTES, FIELD_ELEMENTS_PER_BLOB, PAYLOAD_BYTES, decode_field_elements};
pub use l1::{BlobRef, L1BlockBlobs, L1Client};

/// Endpoints needed to fetch EIP-4844 blobs.
///
/// `l1_rpc` must be a standard EL JSON-RPC that supports `eth_getBlockByNumber`
/// with full transaction bodies; `beacon_rpc` is the consensus-layer HTTP API
/// (e.g. a Lighthouse / Prysm beacon node).
///
/// `verify_kzg` runs `verify_blob_kzg_proof_batch` against the trusted setup
/// before returning. Default is `true` — disable only for local debugging.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    pub l1_rpc: Url,
    pub beacon_rpc: Url,
    pub verify_kzg: bool,
}

impl FetchConfig {
    /// Construct a config with KZG verification enabled.
    pub fn new(l1_rpc: Url, beacon_rpc: Url) -> Self {
        Self { l1_rpc, beacon_rpc, verify_kzg: true }
    }
}

/// One fetched blob, with all the metadata the host needs to populate
/// [`common::GuestInputs`] or write a manifest.
#[derive(Debug, Clone)]
pub struct BlobBundle {
    pub block_number: u64,
    pub tx_index: u64,
    pub blob_index_in_tx: usize,
    pub versioned_hash: [u8; 32],
    pub kzg_commitment: [u8; 48],
    /// Raw 4096 × 32 = 131_072 bytes, exactly as committed on chain.
    pub raw_blob: Box<[u8; BLOB_BYTES]>,
}

/// Fetch every blob committed by the listed `(block_number, tx_index)` refs.
///
/// One bundle per blob; a single tx can carry several. Output order matches
/// the input order, with multi-blob txs flattened in tx-blob order.
///
/// The beacon node is treated as trusted (we don't re-verify KZG proofs), but
/// we still confirm `kzg_to_versioned_hash(commitment) == requested versioned
/// hash` so a buggy / malicious beacon can't substitute a different blob for
/// the one the L1 transaction actually committed to.
pub async fn fetch_blobs(cfg: &FetchConfig, refs: &[BlobRefIn]) -> Result<Vec<BlobBundle>> {
    if refs.is_empty() {
        return Ok(Vec::new());
    }

    let l1 = L1Client::new(cfg.l1_rpc.clone());
    let beacon = BeaconClient::new(cfg.beacon_rpc.clone()).await?;

    tracing::info!(
        genesis_time = beacon.genesis_time,
        seconds_per_slot = beacon.seconds_per_slot,
        "beacon bootstrap"
    );

    // De-dup blocks so we hit each L1 block + beacon slot at most once.
    let mut block_data: BTreeMap<u64, (L1BlockBlobs, Vec<Sidecar>)> = BTreeMap::new();
    for r in refs {
        if block_data.contains_key(&r.block_number) {
            continue;
        }
        let block = l1
            .fetch_block_blobs(r.block_number, None)
            .await
            .with_context(|| format!("fetching L1 block {}", r.block_number))?;
        if block.blobs.is_empty() {
            return Err(anyhow!(
                "L1 block {} carries no blob txs but was requested",
                r.block_number,
            ));
        }
        let slot = beacon.slot_for_timestamp(block.block_timestamp)?;
        let sidecars = beacon
            .get_blob_sidecars(slot)
            .await
            .with_context(|| {
                format!(
                    "fetching sidecars for slot {slot} (L1 block {})",
                    r.block_number
                )
            })?;
        if sidecars.is_empty() {
            return Err(anyhow!(
                "L1 block {} (slot {slot}) referenced blob txs but the beacon \
                 node returned 0 sidecars; is the beacon node out of sync?",
                r.block_number,
            ));
        }
        block_data.insert(r.block_number, (block, sidecars));
    }

    // Walk refs in their original order, emit one BlobBundle per blob.
    // KZG-verify in per-block batches before returning anything: that mirrors
    // how the beacon node delivers sidecars and amortises the c-kzg setup cost
    // across all of a slot's blobs.
    let mut bundles = Vec::new();
    let mut kzg_buffer: BTreeMap<u64, (Vec<[u8; BLOB_BYTES]>, Vec<[u8; 48]>, Vec<[u8; 48]>)> =
        BTreeMap::new();

    for r in refs {
        let (block, sidecars) = block_data
            .get(&r.block_number)
            .expect("inserted above");

        let mut tx_blobs = block
            .blobs
            .iter()
            .filter(|b| b.tx_index == r.tx_index)
            .peekable();

        if tx_blobs.peek().is_none() {
            return Err(anyhow!(
                "tx index {} in L1 block {} carries no blobs",
                r.tx_index,
                r.block_number,
            ));
        }

        for bref in tx_blobs {
            let sc = match_sidecar(bref, sidecars)?;
            if cfg.verify_kzg {
                let entry = kzg_buffer.entry(r.block_number).or_default();
                entry.0.push(*sc.blob);
                entry.1.push(sc.kzg_commitment);
                entry.2.push(sc.kzg_proof);
            }
            bundles.push(BlobBundle {
                block_number: r.block_number,
                tx_index: r.tx_index,
                blob_index_in_tx: bref.blob_index_in_tx,
                versioned_hash: bref.versioned_hash.0,
                kzg_commitment: sc.kzg_commitment,
                raw_blob: sc.blob.clone(),
            });
        }
    }

    if cfg.verify_kzg {
        for (block_number, (blobs, commits, proofs)) in &kzg_buffer {
            kzg::verify_batch(blobs, commits, proofs).with_context(|| {
                format!("KZG batch verification failed for L1 block {block_number}")
            })?;
            tracing::debug!(block = block_number, blobs = blobs.len(), "kzg verified");
        }
    }

    Ok(bundles)
}

/// Convenience wrapper: fetch `refs` and return the concatenated raw bytes,
/// shaped to slot directly into [`common::GuestInputs::blob_data`]. Length is
/// always `BLOB_RAW_BYTES * <number of fetched blobs>`.
pub async fn fetch_blob_data(cfg: &FetchConfig, refs: &[BlobRefIn]) -> Result<Vec<u8>> {
    let bundles = fetch_blobs(cfg, refs).await?;
    let mut out = Vec::with_capacity(BLOB_RAW_BYTES * bundles.len());
    for b in bundles {
        out.extend_from_slice(b.raw_blob.as_ref());
    }
    Ok(out)
}

fn match_sidecar<'a>(blob_ref: &BlobRef, sidecars: &'a [Sidecar]) -> Result<&'a Sidecar> {
    let want = blob_ref.versioned_hash;
    for sc in sidecars {
        let got = kzg_to_versioned_hash(&sc.kzg_commitment);
        if got == want {
            debug_assert_eq!(sc.blob.len(), BLOB_BYTES);
            return Ok(sc);
        }
    }
    Err(anyhow!(
        "no sidecar in slot matched versioned hash {} (tx {})",
        want,
        blob_ref.tx_hash
    ))
}
