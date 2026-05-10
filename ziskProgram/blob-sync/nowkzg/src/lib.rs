//! Standalone EIP-4844 blob sync.
//!
//! Walks an L1 block range, fetches every blob via the beacon node, verifies
//! each sidecar's commitment matches the requested versioned hash, decodes
//! the field-element encoding, and writes the result to disk.
//!
//! This crate intentionally re-implements the slice of op-succinct's blob
//! path that fetches from the beacon node (`utils/host/src/fetcher.rs` +
//! `utils/host/src/witness_generation/online_blob_store.rs`) without dragging
//! in `kona-host`, derivation, or SP1 plumbing — it just reads.

pub mod beacon;
pub mod decode;
pub mod kzg;
pub mod l1;
pub mod sink;

use alloy_eips::eip4844::kzg_to_versioned_hash;
use alloy_primitives::Address;
use anyhow::{Context, Result, anyhow};
use std::path::PathBuf;
use url::Url;

use crate::{
    beacon::{BeaconClient, Sidecar},
    decode::{BLOB_BYTES, decode_field_elements},
    l1::{BlobRef, L1Client},
    sink::DiskSink,
};

#[derive(Debug, Clone)]
pub struct SyncConfig {
    pub l1_rpc: Url,
    pub beacon_rpc: Url,
    pub start_block: u64,
    pub end_block: u64,
    pub batcher: Option<Address>,
    pub out_dir: PathBuf,
    /// If true, strip the leading 0x00 byte from each field element. If false,
    /// the raw 131_072-byte blob is persisted as-is.
    pub decode: bool,
    /// If true, run `verify_blob_kzg_proof_batch` on every block's fetched
    /// blobs before writing them to disk. Mirrors what op-succinct verifies
    /// in-zkVM (`utils/client/src/oracle/blob_provider.rs:28-37`).
    pub verify_kzg: bool,
}

pub async fn sync_range(cfg: SyncConfig) -> Result<()> {
    if cfg.start_block > cfg.end_block {
        return Err(anyhow!(
            "start_block {} > end_block {}",
            cfg.start_block,
            cfg.end_block
        ));
    }

    let l1 = L1Client::new(cfg.l1_rpc.clone());
    let beacon = BeaconClient::new(cfg.beacon_rpc.clone()).await?;
    let sink = DiskSink::new(cfg.out_dir.clone(), cfg.decode);

    tracing::info!(
        genesis_time = beacon.genesis_time,
        seconds_per_slot = beacon.seconds_per_slot,
        "beacon bootstrap"
    );

    let mut total_blobs = 0u64;
    for n in cfg.start_block..=cfg.end_block {
        let block = l1
            .fetch_block_blobs(n, cfg.batcher)
            .await
            .with_context(|| format!("fetching L1 block {n}"))?;

        if block.blobs.is_empty() {
            tracing::debug!(block = n, "no blob txs");
            continue;
        }

        let slot = beacon.slot_for_timestamp(block.block_timestamp)?;
        let sidecars = beacon
            .get_blob_sidecars(slot)
            .await
            .with_context(|| format!("fetching sidecars for slot {slot} (L1 block {n})"))?;

        if sidecars.is_empty() {
            return Err(anyhow!(
                "L1 block {n} (slot {slot}) referenced {} blob(s) but the beacon \
                 node returned 0 sidecars; is the beacon node out of sync, or pre-Cancun?",
                block.blobs.len()
            ));
        }

        // Match each requested versioned hash to its sidecar, then KZG-verify
        // the whole batch in one call before persisting anything.
        let mut matched: Vec<&Sidecar> = Vec::with_capacity(block.blobs.len());
        for blob_ref in &block.blobs {
            matched.push(match_sidecar(blob_ref, &sidecars)?);
        }

        if cfg.verify_kzg {
            let raw_blobs: Vec<[u8; BLOB_BYTES]> = matched.iter().map(|s| *s.blob).collect();
            let commits: Vec<[u8; 48]> = matched.iter().map(|s| s.kzg_commitment).collect();
            let proofs: Vec<[u8; 48]> = matched.iter().map(|s| s.kzg_proof).collect();
            kzg::verify_batch(&raw_blobs, &commits, &proofs).with_context(|| {
                format!("KZG batch verification failed for L1 block {n} (slot {slot})")
            })?;
            tracing::debug!(block = n, slot, blobs = matched.len(), "kzg verified");
        }

        let mut payloads = Vec::with_capacity(matched.len());
        let mut commitments = Vec::with_capacity(matched.len());
        for sidecar in &matched {
            let bytes = if cfg.decode {
                decode_field_elements(&sidecar.blob)?
            } else {
                sidecar.blob.to_vec()
            };
            payloads.push(bytes);
            commitments.push(sidecar.kzg_commitment);
        }

        sink.write_block(
            block.block_number,
            block.block_hash.0,
            block.block_timestamp,
            slot,
            &block.blobs,
            &payloads,
            &commitments,
        )?;

        total_blobs += block.blobs.len() as u64;
        tracing::info!(
            block = n,
            slot,
            blobs = block.blobs.len(),
            "wrote blobs"
        );
    }

    tracing::info!(
        total_blobs,
        out = %sink.root().display(),
        "sync complete"
    );
    Ok(())
}

/// Find the sidecar whose `kzg_to_versioned_hash(commitment)` matches the
/// requested versioned hash. This is the same identity check op-succinct
/// performs in-zkVM (`utils/client/src/oracle/blob_provider.rs:23-26`).
fn match_sidecar<'a>(blob_ref: &BlobRef, sidecars: &'a [Sidecar]) -> Result<&'a Sidecar> {
    let want = blob_ref.versioned_hash;
    for sc in sidecars {
        let got = kzg_to_versioned_hash(&sc.kzg_commitment);
        if got == want {
            // Sanity: the sidecar.blob is exactly 131_072 bytes by construction.
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
