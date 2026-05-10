use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::{SolValue, sol};
use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};

sol! {
    /// Mirror of `OpenBlob.BlobRef`. ABI-encodes identically to the on-chain
    /// struct, so embedding `Vec<BlobRef>` in the public-inputs digest matches
    /// what the contract hashes from calldata.
    struct BlobRef {
        uint64 blockNumber;
        uint64 txIndex;
    }

    /// Public values committed by the OpenBlob ZisK proof.
    ///
    /// `publicInputsHash` is the keccak256 of the same digest the OpenBlob
    /// contract recomputes on-chain (see `OpenBlob.proofBlobDA`). The verifier
    /// compares this against the on-chain digest to bind the proof to a
    /// specific batch.
    ///
    /// `valid` is the circuit's verdict — for now this is a stub that always
    /// returns true; full state-transition checks come later.
    ///
    /// `totalEtherAccumulated` is the saturating big-endian sum of every
    /// `entry_auths[i].ether_amount` the guest receives as private input.
    /// Signature verification of those entries is still a stub (see the guest
    /// TODO); this field commits to the sum the prover claims, so a verifier
    /// can compare it against an on-chain expected total once authentication
    /// is wired up.
    struct Output {
        bytes32 publicInputsHash;
        bool valid;
        bytes32 totalEtherAccumulated;
    }
}

/// Plain-Rust mirror of [`BlobRef`] that round-trips through serde so it can
/// travel across the ZisK stdin boundary without dragging in alloy's serde.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct BlobRefIn {
    pub block_number: u64,
    pub tx_index: u64,
}

/// One signed entry, supplied as a private guest input.
///
/// * `signature` — 65-byte secp256k1 signature in `r || s || v` form over the
///   EIP-191 personal_sign digest of the entry's `data_i` (see
///   [`specs/spec.md`](../../specs/spec.md) §5.1). Currently NOT verified by
///   the guest; the `ecrecover` pass lands once we wire ZisK's secp256k1
///   precompile.
/// * `ether_amount` — the ether (or wei-denominated token amount) the signer
///   commits for this entry, encoded as a 32-byte big-endian unsigned
///   integer matching Solidity's `uint256` ABI layout.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EntryAuth {
    pub signature: [u8; 65],
    pub ether_amount: [u8; 32],
}

/// Inputs to the OpenBlob proof guest, mirroring the fields the OpenBlob
/// contract feeds into its on-chain `keccak256(abi.encode(...))`, plus the
/// private blob data and per-entry authentications (signature + ether
/// amount) that the guest sums.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GuestInputs {
    pub blobs: Vec<BlobRefIn>,
    pub prev_root: [u8; 32],
    pub new_root: [u8; 32],
    pub total_ether_paid: [u8; 32],
    pub block_hash: [u8; 32],
    pub msg_sender: [u8; 20],
    /// `publicInputsHash` recomputed off-chain from the same fields the
    /// OpenBlob contract hashes. The guest re-derives the digest and panics if
    /// it does not match — binding the proof to a specific on-chain batch.
    pub expected_public_inputs_hash: [u8; 32],

    /// Private input. Raw blob bytes for every blob referenced by `blobs`,
    /// concatenated in order. Length MUST equal `BLOB_RAW_BYTES *
    /// blobs.len()`. The guest enforces spec §3 canonicality
    /// (`byte[i*32] == 0x00` for every field element) but does NOT yet RLP-
    /// decode entries from the payload.
    pub blob_data: Vec<u8>,

    /// Private input. One [`EntryAuth`] per signed entry the prover claims
    /// is in the blob. The guest currently only sums the `ether_amount`s
    /// (saturating); signature verification is TODO.
    pub entry_auths: Vec<EntryAuth>,
}

/// Bytes per raw EIP-4844 blob (`FIELD_ELEMENTS_PER_BLOB * BYTES_PER_FIELD`).
pub const BLOB_RAW_BYTES: usize = 4096 * 32;

/// Bytes of usable payload per blob under spec §3 (`4096 * 31`).
pub const BLOB_PAYLOAD_BYTES: usize = 4096 * 31;

/// Recompute `publicInputsHash` from `inputs` using the same ABI-encoding the
/// OpenBlob contract uses on-chain:
/// ```text
/// keccak256(abi.encode(blobs, prevRoot, newRoot, totalEtherPaid,
///                      blockhash(blockNumber), msg.sender))
/// ```
/// Both the host and the guest call this so the digest stays defined in one
/// place. In the guest the `tiny-keccak` Keccak-f routes through ZisK's
/// `syscall_keccak_f` precompile (workspace-level patch).
pub fn compute_public_inputs_hash(inputs: &GuestInputs) -> [u8; 32] {
    let blobs: Vec<BlobRef> = inputs
        .blobs
        .iter()
        .map(|b| BlobRef {
            blockNumber: b.block_number,
            txIndex: b.tx_index,
        })
        .collect();
    let prev_root = B256::from(inputs.prev_root);
    let new_root = B256::from(inputs.new_root);
    let total_ether_paid = U256::from_be_bytes(inputs.total_ether_paid);
    let block_hash = B256::from(inputs.block_hash);
    let msg_sender = Address::from(inputs.msg_sender);

    let encoded = (
        blobs,
        prev_root,
        new_root,
        total_ether_paid,
        block_hash,
        msg_sender,
    )
        .abi_encode_params();

    let mut hasher = Keccak::v256();
    hasher.update(&encoded);
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    out
}

/// Saturating sum of every `ether_amount` in `auths`, returned as a 32-byte
/// big-endian array so it slots straight into `Output.totalEtherAccumulated`
/// (a Solidity `bytes32`).
///
/// Saturation, not wrap: if the running total ever crosses `U256::MAX`, the
/// result clamps. This is intentional — wrapping a "sum of payments" is
/// almost certainly a bug, and saturating gives the verifier a witness it
/// can sanity-check (the value is `U256::MAX` ⇒ overflow happened ⇒ reject).
pub fn sum_ether(auths: &[EntryAuth]) -> [u8; 32] {
    let mut acc = U256::ZERO;
    for a in auths {
        let v = U256::from_be_bytes(a.ether_amount);
        acc = acc.saturating_add(v);
    }
    acc.to_be_bytes()
}

/// Hard-coded example batch used by the host driver and `build.rs`. Lets every
/// host binary (and the proof input file) stay in sync without duplicating the
/// values in five places.
pub fn sample_inputs() -> GuestInputs {
    let blobs = vec![
        BlobRefIn { block_number: 100, tx_index: 0 },
        BlobRefIn { block_number: 100, tx_index: 1 },
    ];
    let prev_root = [0u8; 32];
    let new_root = [0xCCu8; 32];
    let total_ether_paid = [0u8; 32];
    let block_hash = [0xDDu8; 32];
    let msg_sender = [0x42u8; 20];

    // Two zero blobs. The all-zero pattern trivially satisfies §3
    // canonicality (every `chunk[0]` is already 0x00) so the guest's check
    // passes without us emitting real EIP-4844 data here.
    let blob_data = vec![0u8; BLOB_RAW_BYTES * blobs.len()];

    // Two stub entries: 1.5 ETH and 0.5 ETH (in wei). Sum = 2 ETH.
    // Signatures are zero-filled — they're not verified yet. Once the
    // ecrecover stub lands, replace these with real EIP-191 signatures over
    // the entry payloads.
    let amount_a: [u8; 32] = U256::from(1_500_000_000_000_000_000u128).to_be_bytes();
    let amount_b: [u8; 32] = U256::from(500_000_000_000_000_000u128).to_be_bytes();
    let entry_auths = vec![
        EntryAuth { signature: [0u8; 65], ether_amount: amount_a },
        EntryAuth { signature: [0u8; 65], ether_amount: amount_b },
    ];

    let mut inputs = GuestInputs {
        blobs,
        prev_root,
        new_root,
        total_ether_paid,
        block_hash,
        msg_sender,
        expected_public_inputs_hash: [0u8; 32],
        blob_data,
        entry_auths,
    };
    inputs.expected_public_inputs_hash = compute_public_inputs_hash(&inputs);
    inputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_ether_empty_is_zero() {
        assert_eq!(sum_ether(&[]), [0u8; 32]);
    }

    #[test]
    fn sum_ether_two_small_amounts() {
        let a = EntryAuth {
            signature: [0; 65],
            ether_amount: U256::from(7u64).to_be_bytes(),
        };
        let b = EntryAuth {
            signature: [0; 65],
            ether_amount: U256::from(35u64).to_be_bytes(),
        };
        let total = U256::from_be_bytes(sum_ether(&[a, b]));
        assert_eq!(total, U256::from(42u64));
    }

    #[test]
    fn sum_ether_saturates_on_overflow() {
        let max: [u8; 32] = U256::MAX.to_be_bytes();
        let one: [u8; 32] = U256::from(1u64).to_be_bytes();
        let auths = [
            EntryAuth { signature: [0; 65], ether_amount: max },
            EntryAuth { signature: [0; 65], ether_amount: one },
        ];
        assert_eq!(sum_ether(&auths), max, "saturating add must clamp at U256::MAX");
    }

    #[test]
    fn sum_ether_long_chain_below_overflow() {
        // 1000 entries * 1e18 wei each = 1000 ETH. Well below U256::MAX.
        let one_eth: [u8; 32] = U256::from(1_000_000_000_000_000_000u128).to_be_bytes();
        let auths: Vec<EntryAuth> = (0..1000)
            .map(|_| EntryAuth { signature: [0; 65], ether_amount: one_eth })
            .collect();
        let total = U256::from_be_bytes(sum_ether(&auths));
        assert_eq!(total, U256::from(1_000u128) * U256::from(1_000_000_000_000_000_000u128));
    }

    #[test]
    fn sample_inputs_are_self_consistent() {
        let inputs = sample_inputs();

        // The expected hash field round-trips with compute_public_inputs_hash.
        assert_eq!(
            inputs.expected_public_inputs_hash,
            compute_public_inputs_hash(&inputs),
        );

        // blob_data length matches the declared blob count.
        assert_eq!(
            inputs.blob_data.len(),
            BLOB_RAW_BYTES * inputs.blobs.len(),
        );

        // Every field element satisfies §3 canonicality (high byte = 0).
        for chunk in inputs.blob_data.chunks_exact(32) {
            assert_eq!(chunk[0], 0x00);
        }

        // Sample sum is exactly 2 ETH.
        let sum = U256::from_be_bytes(sum_ether(&inputs.entry_auths));
        assert_eq!(sum, U256::from(2_000_000_000_000_000_000u128));
    }

    #[test]
    fn sample_inputs_serialize_round_trip() {
        // Guards the ZisK stdin path: GuestInputs must survive bincode round
        // trips without panicking on the new Vec<u8> / Vec<EntryAuth> fields.
        let inputs = sample_inputs();
        let bytes = bincode::serialize(&inputs).expect("serialize");
        let back: GuestInputs = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(back.blob_data.len(), inputs.blob_data.len());
        assert_eq!(back.entry_auths.len(), inputs.entry_auths.len());
        assert_eq!(
            sum_ether(&back.entry_auths),
            sum_ether(&inputs.entry_auths),
        );
    }
}
