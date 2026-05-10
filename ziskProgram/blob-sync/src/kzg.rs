//! KZG batch verification for fetched blobs.
//!
//! Answers: "given these blob bytes and the (commitment, proof) the beacon
//! node returned, does the proof verify against the trusted KZG setup?"
//! Combined with the `kzg_to_versioned_hash(commitment) == versioned_hash`
//! identity (which the L1 transaction signed), this rules out a malicious or
//! out-of-sync beacon node returning bytes that don't match the on-chain
//! commitment.
//!
//! This is host-only. `c-kzg` is a C library that won't compile for the
//! `riscv64ima-zisk-zkvm-elf` guest target, so verification has to happen
//! before the blob bytes are handed to the ZisK guest as private input.

use alloy_eips::eip4844::env_settings::EnvKzgSettings;
use anyhow::{Context, Result, anyhow};
use c_kzg::{Blob as CkzgBlob, Bytes48};
use std::sync::OnceLock;

fn settings() -> &'static EnvKzgSettings {
    static SETTINGS: OnceLock<EnvKzgSettings> = OnceLock::new();
    SETTINGS.get_or_init(EnvKzgSettings::default)
}

/// Verify a batch of (blob, commitment, proof) tuples in one shot. All slices
/// must be the same length. Errors on length mismatch, malformed input, or a
/// failing verification.
pub fn verify_batch(
    blobs: &[[u8; 131_072]],
    commitments: &[[u8; 48]],
    proofs: &[[u8; 48]],
) -> Result<()> {
    if blobs.is_empty() {
        return Ok(());
    }
    if blobs.len() != commitments.len() || blobs.len() != proofs.len() {
        return Err(anyhow!(
            "length mismatch: {} blobs, {} commitments, {} proofs",
            blobs.len(),
            commitments.len(),
            proofs.len()
        ));
    }

    let ckzg_blobs: Vec<CkzgBlob> = blobs
        .iter()
        .map(|b| CkzgBlob::from_bytes(b).map_err(|e| anyhow!("invalid blob: {e:?}")))
        .collect::<Result<_>>()?;
    let ckzg_commitments: Vec<Bytes48> = commitments
        .iter()
        .map(|c| Bytes48::from_bytes(c).map_err(|e| anyhow!("invalid commitment: {e:?}")))
        .collect::<Result<_>>()?;
    let ckzg_proofs: Vec<Bytes48> = proofs
        .iter()
        .map(|p| Bytes48::from_bytes(p).map_err(|e| anyhow!("invalid proof: {e:?}")))
        .collect::<Result<_>>()?;

    let ok = settings()
        .get()
        .verify_blob_kzg_proof_batch(&ckzg_blobs, &ckzg_commitments, &ckzg_proofs)
        .map_err(|e| anyhow!("c-kzg verify error: {e:?}"))
        .context("KZG batch verification call failed")?;

    if !ok {
        return Err(anyhow!(
            "KZG batch verification rejected {} blob(s) — beacon node returned bytes that don't match the L1-committed commitment",
            blobs.len()
        ));
    }
    Ok(())
}
