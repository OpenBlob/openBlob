# `blob-sync` — host-side EIP-4844 blob fetcher

## 1. Goal

Provide a small library the OpenBlob ZisK host can call to:

1. Walk a list of `(block_number, tx_index)` refs that came from
   `common::BlobRefIn`.
2. Pull each tx's blobs via the beacon node.
3. Verify each sidecar's commitment matches the blob versioned hash the L1
   transaction committed to (`kzg_to_versioned_hash(commitment) == vh`).
4. Hand the raw blob bytes back in the layout
   `common::GuestInputs::blob_data` expects, so the host can feed them to
   the ZisK guest as a private input.

No derivation, no batch decoding, no on-disk witness — this just reads.

## 2. Path through the data

```
BlobRefIn { block_number, tx_index }   ← from common
    │
    ▼
L1Client::fetch_block_blobs(block)     ← alloy-provider, EL RPC
    │  → BlobRef { tx_index, tx_hash, blob_index_in_tx, versioned_hash }
    ▼
BeaconClient::slot_for_timestamp(ts)   ← genesis_time + SECONDS_PER_SLOT
    │
    ▼
BeaconClient::get_blob_sidecars(slot)  ← /eth/v1/beacon/blob_sidecars/{slot}
    │  → Sidecar { blob, kzg_commitment, kzg_proof, … }
    ▼
match by kzg_to_versioned_hash(commitment) == BlobRef.versioned_hash
    │
    ▼
BlobBundle { raw_blob, commitment, versioned_hash, … }
    ▼
fetch_blob_data → Vec<u8> = concat(raw_blob × N)  // length = BLOB_RAW_BYTES * N
```

## 3. Crate layout

```
blob-sync/
  Cargo.toml         # workspace member, library only
  PLAN.md            # this file
  README.md
  src/
    lib.rs           # FetchConfig, BlobBundle, fetch_blobs, fetch_blob_data
    beacon.rs        # BeaconClient (HTTP, slot resolution, sidecar fetch)
    l1.rs            # L1Client (alloy-provider, blob-tx scan)
    decode.rs        # EIP-4844 field-element decoder (host-only helper)
```

## 4. Components

1. **`beacon.rs`** — `BeaconClient` bootstraps once (`/eth/v1/beacon/genesis`
   + `/eth/v1/config/spec`) and caches `genesis_time` and `SECONDS_PER_SLOT`.
   - `slot_for_timestamp(ts)` → `(ts - genesis_time) / SECONDS_PER_SLOT`.
   - `get_blob_sidecars(slot)` → `Vec<Sidecar>` (404 → empty vec).
2. **`l1.rs`** — `L1Client` wraps `alloy_provider::RootProvider`.
   `fetch_block_blobs(n, sender_filter)` returns the L1 block's hash,
   timestamp, and a `Vec<BlobRef>` carrying `tx_index` so callers can pick
   the specific tx they care about.
3. **`decode.rs`** — `decode_field_elements(blob: &[u8; 131_072]) → Vec<u8>`.
   Strips the leading `0x00` byte from each 32-byte field element and
   concatenates the remaining 31 bytes. Errors on non-canonical high bytes.
4. **`lib.rs`** — public API:
   - `FetchConfig { l1_rpc, beacon_rpc }`
   - `BlobBundle { block_number, tx_index, blob_index_in_tx, versioned_hash,
     kzg_commitment, raw_blob }`
   - `async fetch_blobs(cfg, &[BlobRefIn]) -> Vec<BlobBundle>`
   - `async fetch_blob_data(cfg, &[BlobRefIn]) -> Vec<u8>`

## 5. What we deliberately drop

- **KZG proof verification.** The beacon node is trusted; commitment ↔
  versioned-hash matching is enough. Re-add `c-kzg` and
  `verify_blob_kzg_proof_batch` if a stricter check is required.
- **CLI binary.** The crate used to ship a `blob-sync` binary; it has been
  removed in favour of the in-memory library API the host calls.
- **Disk sink.** No more `out/<block>/<vh>.bin` writes — the host streams
  blob bytes straight into `GuestInputs::blob_data`.

## 6. Build & smoke test

```sh
cargo check -p blob-sync
cargo build -p blob-sync --release
```

Programmatic smoke test from the host (illustrative):

```rust
use blob_sync::{FetchConfig, fetch_blob_data};
use common::BlobRefIn;
use url::Url;

let cfg = FetchConfig {
    l1_rpc:     Url::parse(&std::env::var("L1_RPC")?)?,
    beacon_rpc: Url::parse(&std::env::var("L1_BEACON_RPC")?)?,
};
let refs = vec![BlobRefIn { block_number: 19_426_587, tx_index: 0 }];
let bytes = fetch_blob_data(&cfg, &refs).await?;
assert_eq!(bytes.len() % 131_072, 0);
```

## 7. Extension points

| Want                          | Where                                                    |
| ----------------------------- | -------------------------------------------------------- |
| Strict KZG proof verification | `lib.rs` after `match_sidecar`, before returning bundle  |
| Postgres / S3 cache           | New module that wraps `fetch_blobs`                      |
| Concurrent block fetches      | `fetch_blobs` — replace sequential loop with `buffer_unordered` |
| Follow-tip mode               | New helper that polls `L1Client::latest_block_number`    |
