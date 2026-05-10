# blob-sync

Host-side EIP-4844 blob fetcher used by the OpenBlob ZisK host. For each
`(block_number, tx_index)` ref the host wants to prove over, `blob-sync`:

1. Pulls the L1 block via JSON-RPC and finds the type-3 (blob) transaction at
   the given index.
2. Resolves the matching beacon slot from the L1 block timestamp.
3. Fetches the slot's blob sidecars from the consensus-layer HTTP API.
4. Confirms `kzg_to_versioned_hash(commitment) == blob_versioned_hash` — i.e.
   that the beacon node returned the blob the L1 transaction actually
   committed to.
5. Returns the raw 131_072-byte blob bytes (or the concatenated buffer) ready
   to slot into [`common::GuestInputs::blob_data`].

This crate is **host-only**: it speaks HTTP and uses `tokio` / `reqwest`, so
it cannot link into the ZisK guest. The pure decode helpers in
[`decode`](src/decode.rs) (4096 × 32 → 4096 × 31 byte payload) are
no-network code that the guest can reuse if needed.

## Usage from the host

```rust
use blob_sync::{FetchConfig, fetch_blob_data};
use common::{BlobRefIn, GuestInputs};
use url::Url;

let cfg = FetchConfig {
    l1_rpc:     Url::parse(&std::env::var("L1_RPC")?)?,
    beacon_rpc: Url::parse(&std::env::var("L1_BEACON_RPC")?)?,
};

let blobs = vec![
    BlobRefIn { block_number: 19_426_587, tx_index: 0 },
];

// One concatenated buffer — exactly `BLOB_RAW_BYTES * blobs.len()`.
let blob_data = fetch_blob_data(&cfg, &blobs).await?;

let mut inputs: GuestInputs = /* … fill prev_root, new_root, etc. … */;
inputs.blobs    = blobs;
inputs.blob_data = blob_data;
```

If the host needs per-blob metadata (commitments, versioned hashes, etc.),
call [`fetch_blobs`] instead of [`fetch_blob_data`]; it returns
`Vec<BlobBundle>` with the same data plus the KZG commitment, versioned
hash, and originating tx index.

## Endpoints

| Variable          | Purpose                                                                   |
| ----------------- | ------------------------------------------------------------------------- |
| `L1_RPC`          | EL JSON-RPC. Must support `eth_getBlockByNumber` with full transactions.  |
| `L1_BEACON_RPC`   | Consensus-layer HTTP API (e.g. Lighthouse / Prysm beacon node).           |

## What's deliberately *not* here

- **Strict KZG proof verification.** We accept the beacon node as trusted and
  rely on `kzg_to_versioned_hash(commitment) == requested versioned hash`.
  Add `c-kzg` and call `verify_blob_kzg_proof_batch` if you need the stronger
  guarantee.
- **Disk persistence.** The previous CLI shipped a `DiskSink` that wrote
  blobs to `out/<block>/<vh>.bin` plus a manifest; that lived only to support
  the standalone CLI. The host now consumes blobs in memory.
- **Frame / batch decoding.** This crate returns raw blob bytes only. Any
  rollup-specific framing (length prefix, channel id, etc.) is left to the
  guest.

## File map

| File          | Purpose                                                          |
| ------------- | ---------------------------------------------------------------- |
| `src/lib.rs`  | `FetchConfig`, `BlobBundle`, `fetch_blobs`, `fetch_blob_data`.   |
| `src/beacon.rs` | Beacon HTTP client (`/eth/v1/beacon/{genesis,blob_sidecars}`). |
| `src/l1.rs`   | alloy provider that walks L1 blocks and finds blob txs.          |
| `src/decode.rs` | EIP-4844 field-element decode (4096 × 32 → 4096 × 31).         |
