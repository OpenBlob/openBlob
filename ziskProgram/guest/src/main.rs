// OpenBlob ZisK proof — stub circuit.
//
// Reads:
//   * the public-inputs fields OpenBlob.proofBlobDA hashes on-chain, and
//     re-derives `publicInputsHash = keccak256(abi.encode(...))` to bind
//     this proof to a specific contract digest;
//   * `blob_data` — raw EIP-4844 blob bytes (private input). Verified for
//     spec §3 canonicality (`F[i][0] == 0x00` per 32-byte field element).
//   * `entry_auths` — one `(signature, ether_amount)` pair per signed entry
//     the prover claims is in the blob (private input).
//
// Computes:
//   * `totalEtherAccumulated` = saturating big-endian sum of every
//     `entry_auths[i].ether_amount`.
//
// Commits `Output { publicInputsHash, valid, totalEtherAccumulated }`.
//
// TODO(zk): verify each `signature` against the EIP-191 personal_sign
// digest of the corresponding entry's `data_i` once ZisK's secp256k1
// precompile path is wired up. Until then `valid` is a stub `true` and the
// summed total is unauthenticated — the proof commits to whatever the
// prover claims.
// TODO(zk): RLP-decode entries from `blob_data` and bind each
// `entry_auths[i]` to the i-th decoded `data_i`. Until then there is no
// link between the blob bytes and the per-entry input vector.

#![no_main]
ziskos::entrypoint!(main);

use alloy_sol_types::SolValue;
use common::{
    BLOB_RAW_BYTES, GuestInputs, Output, compute_public_inputs_hash, sum_ether,
};

fn main() {
    let inputs: GuestInputs = ziskos::io::read();

    // Mirror OpenBlob.proofBlobDA's `keccak256(abi.encode(...))` and bind the
    // proof to the contract's hash. If the caller passes inputs that do not
    // hash to the digest the contract will recompute on-chain, fail loudly so
    // the proof is never generated against a mismatched batch.
    let public_inputs_hash = compute_public_inputs_hash(&inputs);
    assert_eq!(
        public_inputs_hash, inputs.expected_public_inputs_hash,
        "publicInputsHash mismatch with on-chain digest",
    );

    // Spec §3 canonicality on the private blob_data: every 32-byte field
    // element must have its high byte zeroed, otherwise the value can exceed
    // BLS_MODULUS and the blob is not a valid EIP-4844 input. Cheap to
    // check, and prevents a malicious prover from feeding a non-canonical
    // payload that would never have made it on-chain.
    assert_eq!(
        inputs.blob_data.len(),
        BLOB_RAW_BYTES * inputs.blobs.len(),
        "blob_data length must equal BLOB_RAW_BYTES * blobs.len()",
    );
    for (i, chunk) in inputs.blob_data.chunks_exact(32).enumerate() {
        assert_eq!(
            chunk[0], 0x00,
            "blob field element {i} violates §3 canonicality (high byte != 0x00)",
        );
    }

    // Sum per-signature ether amounts. Saturating add — see `sum_ether`'s
    // doc for why wrapping would be a bug.
    let total_ether_accumulated = sum_ether(&inputs.entry_auths);

    let output = Output {
        publicInputsHash: public_inputs_hash.into(),
        valid: true,
        totalEtherAccumulated: total_ether_accumulated.into(),
    };

    println!("publicInputsHash:       {:02x?}", public_inputs_hash);
    println!("totalEtherAccumulated:  {:02x?}", total_ether_accumulated);
    println!("entries summed:         {}", inputs.entry_auths.len());

    ziskos::io::commit_slice(&output.abi_encode());
}
