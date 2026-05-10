// OpenBlob ZisK proof.
//
// Reads:
//   * the public-inputs fields OpenBlob.proofBlobDA hashes on-chain, and
//     re-derives `publicInputsHash = keccak256(abi.encode(...))` to bind
//     this proof to a specific contract digest;
//   * `blob_data` — raw EIP-4844 blob bytes for every blob in the producing
//     transaction, concatenated in order (private input);
//   * `entry_auths` — one (signature, ether_amount) pair per signed entry
//     the prover claims is in the cross-blob stream `S` (private input).
//
// Verifies (panics on any deviation):
//   1. `expected_public_inputs_hash` matches the recomputed digest.
//   2. Spec §3 canonicality on every field element of `blob_data`.
//   3. `blob_data.len() == BLOB_RAW_BYTES * blobs.len()`.
//   4. Spec §3.0 logical stream `S` RLP-decodes (canonically) into a list
//      of `k` non-empty `data_i` entries.
//   5. `k == entry_auths.len()`.
//   6. For each i, `entry_auths[i]` is a valid EIP-191 personal_sign
//      signature over `data_i` with `v ∈ {27, 28}` and low-`s`. Recovery
//      runs through ZisK's secp256k1 precompile (workspace patch on
//      `k256`).
//
// Computes:
//   * `totalEtherAccumulated` = saturating sum of every
//     `entry_auths[i].ether_amount`.
//   * `signersHash` = `keccak256(addr_0 || ... || addr_{k-1})` over the
//     20-byte addresses recovered from each entry's signature, in entry
//     order. Lets a verifier match the signer set against an off-chain
//     allow-list (§5.2 step 3) without exposing each address publicly.
//
// Commits `Output { publicInputsHash, valid: true, totalEtherAccumulated,
// signersHash }`.

#![no_main]
ziskos::entrypoint!(main);

use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use common::{
    GuestInputs, Output, compute_public_inputs_hash, decode_entries, keccak_signers,
    sum_ether, unpack_blobs,
};

fn main() {
    let inputs: GuestInputs = ziskos::io::read();

    // 1. Bind the proof to OpenBlob.proofBlobDA's `keccak256(abi.encode(...))`.
    let public_inputs_hash = compute_public_inputs_hash(&inputs);
    assert_eq!(
        public_inputs_hash, inputs.expected_public_inputs_hash,
        "publicInputsHash mismatch with on-chain digest",
    );

    // 2 + 3. §3.0 cross-blob stream. `unpack_blobs` enforces blob_data
    // length and §3 canonicality on every field element.
    let s = unpack_blobs(&inputs.blob_data, inputs.blobs.len());

    // 4. §4 RLP-decode the cross-blob stream into entries.
    let entries = decode_entries(&s);

    // 5. Each declared auth must line up with a decoded entry.
    assert_eq!(
        entries.len(),
        inputs.entry_auths.len(),
        "entry_auths.len() ({}) != decoded k ({})",
        inputs.entry_auths.len(),
        entries.len(),
    );

    // 6. §5 ECDSA recovery on each (data_i, sig_i) pair. `recover` enforces
    // v ∈ {27, 28} and low-`s`, and panics on any malformed signature; a
    // successful recovery binds the entry to *some* signer address.
    // Recovered addresses are collected and hashed into `signersHash` so
    // the §5.2 step 3 application-policy check can run off-chain against an
    // allow-list / registry by hashing that list the same way and matching.
    let mut signers: Vec<Address> = Vec::with_capacity(entries.len());
    for (data, auth) in entries.iter().zip(&inputs.entry_auths) {
        signers.push(auth.recover(data));
    }

    // Sum per-signature ether amounts (saturating).
    let total_ether_accumulated = sum_ether(&inputs.entry_auths);

    // Commit the recovered signer set as a single keccak256 digest.
    let signers_hash = keccak_signers(&signers);

    let output = Output {
        publicInputsHash: public_inputs_hash.into(),
        valid: true,
        totalEtherAccumulated: total_ether_accumulated.into(),
        signersHash: signers_hash.into(),
    };

    println!("publicInputsHash:       {:02x?}", public_inputs_hash);
    println!("totalEtherAccumulated:  {:02x?}", total_ether_accumulated);
    println!("signersHash:            {:02x?}", signers_hash);
    println!("entries verified:       {}", inputs.entry_auths.len());

    ziskos::io::commit_slice(&output.abi_encode());
}
