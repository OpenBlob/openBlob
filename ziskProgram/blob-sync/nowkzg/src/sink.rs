//! On-disk persistence for fetched blobs.

use crate::l1::BlobRef;
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct BlobRecord {
    pub l1_block: u64,
    pub l1_block_hash: String,
    pub l1_block_timestamp: u64,
    pub beacon_slot: u64,
    pub tx_hash: String,
    pub tx_from: String,
    pub blob_index_in_tx: usize,
    pub versioned_hash: String,
    pub kzg_commitment: String,
    pub byte_length: usize,
    pub file: String,
}

#[derive(Debug, Serialize)]
struct BlockManifest {
    l1_block: u64,
    l1_block_hash: String,
    l1_block_timestamp: u64,
    beacon_slot: u64,
    blobs: Vec<BlobRecord>,
}

pub struct DiskSink {
    root: PathBuf,
    decode: bool,
}

impl DiskSink {
    pub fn new(root: impl Into<PathBuf>, decode: bool) -> Self {
        Self { root: root.into(), decode }
    }

    /// Write all blobs for one L1 block, plus a per-block JSON manifest. The
    /// caller passes parallel slices: `refs[i]` describes the source tx/blob,
    /// `payloads[i]` is what to persist (raw 131_072 bytes if `decode == false`,
    /// 126_976 bytes otherwise), `commitments[i]` is the KZG commitment hex.
    pub fn write_block(
        &self,
        block_number: u64,
        block_hash: [u8; 32],
        block_timestamp: u64,
        beacon_slot: u64,
        refs: &[BlobRef],
        payloads: &[Vec<u8>],
        commitments: &[[u8; 48]],
    ) -> Result<()> {
        assert_eq!(refs.len(), payloads.len());
        assert_eq!(refs.len(), commitments.len());

        let block_dir = self.root.join(block_number.to_string());
        fs::create_dir_all(&block_dir)
            .with_context(|| format!("creating {}", block_dir.display()))?;

        let mut records = Vec::with_capacity(refs.len());
        for ((blob_ref, bytes), commit) in refs.iter().zip(payloads.iter()).zip(commitments.iter()) {
            let vh_hex = hex_with_prefix(blob_ref.versioned_hash.as_slice());
            let ext = if self.decode { "bin" } else { "blob" };
            let file_name = format!("{vh_hex}.{ext}");
            let file_path = block_dir.join(&file_name);
            let mut f = File::create(&file_path)
                .with_context(|| format!("creating {}", file_path.display()))?;
            f.write_all(bytes)?;

            records.push(BlobRecord {
                l1_block: block_number,
                l1_block_hash: hex_with_prefix(&block_hash),
                l1_block_timestamp: block_timestamp,
                beacon_slot,
                tx_hash: hex_with_prefix(blob_ref.tx_hash.as_slice()),
                tx_from: format!("{:#x}", blob_ref.tx_from),
                blob_index_in_tx: blob_ref.blob_index_in_tx,
                versioned_hash: vh_hex,
                kzg_commitment: hex_with_prefix(commit),
                byte_length: bytes.len(),
                file: file_name,
            });
        }

        let manifest = BlockManifest {
            l1_block: block_number,
            l1_block_hash: hex_with_prefix(&block_hash),
            l1_block_timestamp: block_timestamp,
            beacon_slot,
            blobs: records,
        };
        let manifest_path = block_dir.join("manifest.json");
        let manifest_json = serde_json::to_vec_pretty(&manifest)?;
        fs::write(&manifest_path, manifest_json)
            .with_context(|| format!("writing {}", manifest_path.display()))?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn hex_with_prefix(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("0x");
    s.push_str(&hex::encode(bytes));
    s
}
