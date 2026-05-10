use anyhow::Result;
use blob_sync::{SyncConfig, sync_range};
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;
use url::Url;

#[derive(Debug, Parser)]
#[command(
    name = "blob-sync",
    about = "Walk an L1 block range, pull every EIP-4844 blob from a beacon \
             node, decode the field-element encoding, and write to disk."
)]
struct Cli {
    /// L1 execution-layer JSON-RPC URL.
    #[arg(long, env = "L1_RPC")]
    l1_rpc: Url,

    /// L1 consensus-layer (beacon) HTTP API URL.
    #[arg(long, env = "L1_BEACON_RPC")]
    beacon_rpc: Url,

    /// First L1 block (inclusive) to scan.
    #[arg(long)]
    start: u64,

    /// Last L1 block (inclusive) to scan.
    #[arg(long)]
    end: u64,

    /// Optional batcher / sender filter — only fetch blobs from txs whose
    /// `from` matches this address. Mirrors how rollup derivation finds its
    /// data.
    #[arg(long)]
    batcher: Option<alloy_primitives::Address>,

    /// Output directory. One subdir per L1 block, plus a per-block
    /// `manifest.json`.
    #[arg(long, default_value = "./out")]
    out: PathBuf,

    /// Skip the EIP-4844 field-element decode and write raw 131_072-byte
    /// blobs verbatim.
    #[arg(long)]
    no_decode: bool,

    /// Skip cryptographic KZG batch verification of fetched blobs. By default
    /// every blob is checked against its commitment + proof
    /// (`verify_blob_kzg_proof_batch`); pass this flag if you trust the
    /// beacon node and want a faster sync.
    #[arg(long)]
    no_kzg_verify: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    let cfg = SyncConfig {
        l1_rpc: cli.l1_rpc,
        beacon_rpc: cli.beacon_rpc,
        start_block: cli.start,
        end_block: cli.end,
        batcher: cli.batcher,
        out_dir: cli.out,
        decode: !cli.no_decode,
        verify_kzg: !cli.no_kzg_verify,
    };
    sync_range(cfg).await
}
