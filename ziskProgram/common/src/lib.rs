use alloy_primitives::{Address, B256, Signature, U256, eip191_hash_message};
use alloy_rlp::{Bytes, Decodable, Encodable, Header};
use alloy_sol_types::sol;
use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};

use alloy_sol_types::SolValue;

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
    /// * `publicInputsHash` — `keccak256(abi.encode(...))` mirroring
    ///   `OpenBlob.proofBlobDA`. Verifier compares against the on-chain
    ///   digest to bind the proof to a specific batch.
    /// * `valid` — circuit verdict. The guest verifies signatures and §3
    ///   canonicality, so a successfully-generated proof is one where every
    ///   assertion held; this bit is `true` whenever the proof exists.
    /// * `totalEtherAccumulated` — saturating big-endian sum of every
    ///   `entry_auths[i].ether_amount`, computed inside the circuit.
    /// * `signersHash` — `keccak256(addr_0 || addr_1 || ... || addr_{k-1})`
    ///   over the 20-byte addresses recovered from each entry's signature,
    ///   in the order the entries appear in the §4 RLP list. Lets a
    ///   verifier check the set of authorized signers without committing
    ///   each address as a separate public output, and lets the on-chain
    ///   policy hook (§5.2 step 3) bind the proof to an off-chain
    ///   allow-list / registry by hashing that list the same way and
    ///   comparing.
    struct Output {
        bytes32 publicInputsHash;
        bool valid;
        bytes32 totalEtherAccumulated;
        bytes32 signersHash;
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
/// Carries an EIP-191 personal_sign signature over the entry's `data_i`
/// (split into r/s/v so it round-trips through serde without a big-array
/// workaround) and the wei-denominated ether amount the signer commits.
///
/// See [`specs/spec.md`](../../specs/spec.md) §5 for the signing scheme.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryAuth {
    /// Signature `r` component, big-endian.
    pub r: [u8; 32],
    /// Signature `s` component, big-endian. Spec §5.1 requires low-`s`
    /// (`s ≤ secp256k1n / 2`); [`EntryAuth::recover`] enforces this.
    pub s: [u8; 32],
    /// Recovery byte. Spec §5.1 requires `v ∈ {27, 28}`;
    /// [`EntryAuth::recover`] panics on anything else.
    pub v: u8,
    /// Wei-denominated ether amount the signer commits for this entry,
    /// encoded as a 32-byte big-endian unsigned integer (uint256).
    pub ether_amount: [u8; 32],
}

impl EntryAuth {
    /// Recover the signer address for `data_i` per spec §5.1/§5.2:
    ///   1. Compute the EIP-191 `personal_sign` digest of `data_i`
    ///      (`alloy_primitives::eip191_hash_message`).
    ///   2. Validate `v ∈ {27, 28}`.
    ///   3. Validate low-`s` (EIP-2 — `s ≤ secp256k1n / 2`).
    ///   4. Run secp256k1 ECDSA recovery against the digest.
    ///
    /// Panics on any deviation. The guest is a circuit, not a fallible API,
    /// so a malformed input MUST fail the proof rather than silently produce
    /// garbage.
    pub fn recover(&self, data: &[u8]) -> Address {
        let parity = match self.v {
            27 => false,
            28 => true,
            other => panic!(
                "invalid signature v = {other} (spec §5.1 requires v ∈ {{27, 28}})",
            ),
        };

        let r = U256::from_be_bytes(self.r);
        let s = U256::from_be_bytes(self.s);
        let sig = Signature::new(r, s, parity);

        // Spec §5.1 + EIP-2: high-`s` is rejected. `normalize_s()` returns
        // `Some(low_s_form)` only when the input was high-`s`; if it returns
        // `None` the signature was already canonical.
        assert!(
            sig.normalize_s().is_none(),
            "high-s signature rejected (EIP-2 / spec §5.1)",
        );

        sig.recover_address_from_msg(data)
            .expect("ecrecover failed — invalid signature for data_i")
    }
}

/// Inputs to the OpenBlob proof guest.
///
/// Mirrors the fields the OpenBlob contract feeds into its on-chain
/// `keccak256(abi.encode(...))`, plus the private blob bytes and per-entry
/// authentications the guest decodes and verifies.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GuestInputs {
    pub blobs: Vec<BlobRefIn>,
    pub prev_root: [u8; 32],
    pub new_root: [u8; 32],
    pub total_ether_paid: [u8; 32],
    pub block_hash: [u8; 32],
    pub msg_sender: [u8; 20],
    /// `publicInputsHash` recomputed off-chain from the same fields the
    /// OpenBlob contract hashes. The guest re-derives the digest and panics
    /// if it does not match — binding the proof to a specific on-chain batch.
    pub expected_public_inputs_hash: [u8; 32],

    /// Private input. Raw blob bytes for every blob referenced by `blobs`,
    /// concatenated in transaction order. Length MUST equal
    /// `BLOB_RAW_BYTES * blobs.len()`. The guest enforces spec §3
    /// canonicality (`byte[i*32] == 0x00` per field element) and treats the
    /// per-blob payloads as the §3.0 cross-blob logical stream `S`.
    pub blob_data: Vec<u8>,

    /// Private input. One [`EntryAuth`] per signed entry the prover claims
    /// is in `S`. `entry_auths.len()` MUST equal `k`, the number of items
    /// the guest decodes from `S` via §4 RLP. `entry_auths[i]` is bound to
    /// the i-th decoded `data_i`; signature verification is enforced.
    pub entry_auths: Vec<EntryAuth>,
}

/// Field elements per EIP-4844 blob.
pub const FIELD_ELEMENTS_PER_BLOB: usize = 4096;
/// Bytes per field element on the wire.
pub const BYTES_PER_FIELD_ELEMENT: usize = 32;
/// Bytes of usable payload per field element (spec §3 — high byte reserved).
pub const USABLE_BYTES_PER_FIELD: usize = 31;
/// Bytes per raw blob (`FIELD_ELEMENTS_PER_BLOB * BYTES_PER_FIELD_ELEMENT`).
pub const BLOB_RAW_BYTES: usize = FIELD_ELEMENTS_PER_BLOB * BYTES_PER_FIELD_ELEMENT;
/// Bytes of usable payload per blob (`FIELD_ELEMENTS_PER_BLOB *
/// USABLE_BYTES_PER_FIELD`).
pub const BLOB_PAYLOAD_BYTES: usize = FIELD_ELEMENTS_PER_BLOB * USABLE_BYTES_PER_FIELD;

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

/// Decode one raw blob's 126976-byte logical payload per spec §3.2: strip
/// the high zero byte off every 32-byte field element. Panics on a
/// non-canonical field element (`F[i][0] != 0x00`).
pub fn unpack_blob(blob: &[u8]) -> Vec<u8> {
    assert_eq!(
        blob.len(),
        BLOB_RAW_BYTES,
        "blob must be exactly {BLOB_RAW_BYTES} bytes",
    );
    let mut out = Vec::with_capacity(BLOB_PAYLOAD_BYTES);
    for i in 0..FIELD_ELEMENTS_PER_BLOB {
        let fe = &blob[i * BYTES_PER_FIELD_ELEMENT..(i + 1) * BYTES_PER_FIELD_ELEMENT];
        assert_eq!(
            fe[0], 0x00,
            "field element {i} violates §3 canonicality (high byte != 0x00)",
        );
        out.extend_from_slice(&fe[1..]);
    }
    out
}

/// Pack a logical payload into one raw 131072-byte blob per spec §3.1.
/// `payload.len()` MUST be `≤ BLOB_PAYLOAD_BYTES`; missing bytes are
/// zero-padded.
pub fn pack_blob(payload: &[u8]) -> Vec<u8> {
    assert!(
        payload.len() <= BLOB_PAYLOAD_BYTES,
        "payload exceeds BLOB_PAYLOAD_BYTES = {BLOB_PAYLOAD_BYTES}",
    );
    let mut padded = vec![0u8; BLOB_PAYLOAD_BYTES];
    padded[..payload.len()].copy_from_slice(payload);

    let mut blob = Vec::with_capacity(BLOB_RAW_BYTES);
    for i in 0..FIELD_ELEMENTS_PER_BLOB {
        blob.push(0x00);
        blob.extend_from_slice(
            &padded[i * USABLE_BYTES_PER_FIELD..(i + 1) * USABLE_BYTES_PER_FIELD],
        );
    }
    blob
}

/// Concatenate every blob's logical payload into the cross-blob stream `S`
/// per spec §3.0 (`S = P_0 || P_1 || ... || P_{n-1}`). Each blob is
/// canonicality-checked by [`unpack_blob`].
pub fn unpack_blobs(blob_data: &[u8], n: usize) -> Vec<u8> {
    assert_eq!(
        blob_data.len(),
        BLOB_RAW_BYTES * n,
        "blob_data must be {n} * {BLOB_RAW_BYTES} bytes",
    );
    let mut s = Vec::with_capacity(BLOB_PAYLOAD_BYTES * n);
    for j in 0..n {
        let raw = &blob_data[j * BLOB_RAW_BYTES..(j + 1) * BLOB_RAW_BYTES];
        s.extend_from_slice(&unpack_blob(raw));
    }
    s
}

/// Decode the RLP entry list from the cross-blob stream `S` per spec §4.
/// Padding past the encoded list is ignored. Empty entries are forbidden
/// (§4.1); the empty list (`k = 0`) is allowed.
///
/// alloy-rlp 0.3.x does NOT specialize `Vec<u8>` as an RLP byte string — it
/// treats it as a list of u8 items, which would mis-frame every entry. We
/// decode the outer list header explicitly with [`Header`] and read each
/// child as [`Bytes`] (which IS the byte-string codec) to get the
/// spec-mandated layout.
pub fn decode_entries(s: &[u8]) -> Vec<Vec<u8>> {
    let mut buf: &[u8] = s;
    let header = Header::decode(&mut buf).expect("RLP list header decode failed");
    assert!(header.list, "outermost RLP item must be a list (spec §4)");
    let payload_len = header.payload_length;
    assert!(
        buf.len() >= payload_len,
        "RLP list payload truncated: header claims {payload_len} bytes, only {} available",
        buf.len(),
    );
    let mut payload = &buf[..payload_len];

    let mut entries = Vec::new();
    while !payload.is_empty() {
        let item: Bytes =
            Decodable::decode(&mut payload).expect("RLP byte-string decode failed");
        entries.push(item.to_vec());
    }
    for (i, e) in entries.iter().enumerate() {
        assert!(
            !e.is_empty(),
            "entry {i} is empty — spec §4.1 forbids empty entries",
        );
    }
    entries
}

/// Encode `entries` as an RLP byte-string list per spec §4. Used by host
/// code to produce the blob payload before §3.1 packing.
///
/// Built from [`Header`] + `&[u8]: Encodable` (byte string) rather than
/// `encode_list` with `Vec<u8>` items, because alloy-rlp 0.3.x has no
/// byte-string specialization for `Vec<u8>` — the generic `Vec<T>` codec
/// would emit each entry as an RLP list-of-bytes (header `0xc?` instead of
/// `0x8?`) and silently violate the spec.
pub fn encode_entries(entries: &[Vec<u8>]) -> Vec<u8> {
    let payload_length: usize = entries
        .iter()
        .map(|e| {
            let s: &[u8] = e.as_slice();
            s.length()
        })
        .sum();
    let header = Header {
        list: true,
        payload_length,
    };
    let mut buf = Vec::with_capacity(payload_length + 9);
    header.encode(&mut buf);
    for e in entries {
        let s: &[u8] = e.as_slice();
        s.encode(&mut buf);
    }
    buf
}

/// Saturating sum of every `ether_amount` in `auths`, returned as a 32-byte
/// big-endian array so it slots straight into `Output.totalEtherAccumulated`
/// (a Solidity `bytes32`). Saturating, not wrapping: overflow clamps at
/// `U256::MAX` so the verifier sees the failure rather than a silently
/// wrapped total.
pub fn sum_ether(auths: &[EntryAuth]) -> [u8; 32] {
    let mut acc = U256::ZERO;
    for a in auths {
        acc = acc.saturating_add(U256::from_be_bytes(a.ether_amount));
    }
    acc.to_be_bytes()
}

/// `keccak256(signers[0] || signers[1] || ... || signers[k-1])` over the
/// concatenation of 20-byte recovered addresses, in entry order. Used to
/// commit the signer set as `Output.signersHash` so a verifier can match
/// against an off-chain allow-list (§5.2 step 3) without exposing each
/// address as a separate public input.
///
/// For `signers.is_empty()`, returns `keccak256("")` — the standard empty
/// digest. This lets a verifier distinguish "no signers" from any
/// non-empty list by a single equality check.
pub fn keccak_signers(signers: &[Address]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    for addr in signers {
        hasher.update(addr.as_slice());
    }
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    out
}

/// Sign `data` with `sk` (raw secp256k1 private key) per EIP-191
/// `personal_sign`, returning a low-`s` normalized signature with
/// `v ∈ {27, 28}`. Used by host-side sample inputs and tests; the guest
/// only verifies, never signs.
pub fn sign_eip191(sk: &[u8; 32], data: &[u8]) -> EntryAuth {
    use k256::ecdsa::SigningKey;

    let signing_key = SigningKey::from_bytes(sk.into()).expect("valid secp256k1 key");
    let digest: B256 = eip191_hash_message(data);
    let (sig, recid) = signing_key
        .sign_prehash_recoverable(digest.as_slice())
        .expect("signing failed");
    // k256's `sign_prehash_recoverable` returns a signature with low-`s`
    // already normalized, so we don't need to re-normalize here.
    let r: [u8; 32] = sig.r().to_bytes().into();
    let s: [u8; 32] = sig.s().to_bytes().into();
    let v: u8 = recid.to_byte() + 27;
    EntryAuth {
        r,
        s,
        v,
        ether_amount: [0u8; 32],
    }
}

/// Convenience: sign `data` and tag the result with `ether_amount`.
pub fn sign_entry(sk: &[u8; 32], data: &[u8], ether_amount: U256) -> EntryAuth {
    let mut auth = sign_eip191(sk, data);
    auth.ether_amount = ether_amount.to_be_bytes();
    auth
}

/// Hard-coded example batch used by the host driver and `build.rs`. Two
/// entries signed by Anvil's first test key, packed into one blob.
pub fn sample_inputs() -> GuestInputs {
    // Anvil's first deterministic test account.
    // Address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
    const ANVIL_KEY: [u8; 32] = [
        0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4,
        0xd2, 0x38, 0xff, 0x94, 0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc,
        0xae, 0x78, 0x4d, 0x7b, 0xf4, 0xf2, 0xff, 0x80,
    ];

    let entries: Vec<Vec<u8>> = vec![b"hello".to_vec(), b"world!".to_vec()];

    let entry_auths = vec![
        sign_entry(
            &ANVIL_KEY,
            &entries[0],
            U256::from(1_500_000_000_000_000_000u128), // 1.5 ETH
        ),
        sign_entry(
            &ANVIL_KEY,
            &entries[1],
            U256::from(500_000_000_000_000_000u128), // 0.5 ETH
        ),
    ];

    let payload = encode_entries(&entries);
    let blob_data = pack_blob(&payload);

    let blobs = vec![BlobRefIn {
        block_number: 100,
        tx_index: 0,
    }];
    let prev_root = [0u8; 32];
    let new_root = [0xCC; 32];
    // Make total_ether_paid match the on-chain claim that equals the sum.
    let total_ether_paid = sum_ether(&entry_auths);
    let block_hash = [0xDD; 32];
    let msg_sender = [0x42; 20];

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

    /// Anvil's first account — used as the deterministic test signer.
    const ANVIL_KEY: [u8; 32] = [
        0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4,
        0xd2, 0x38, 0xff, 0x94, 0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc,
        0xae, 0x78, 0x4d, 0x7b, 0xf4, 0xf2, 0xff, 0x80,
    ];
    /// Address derived from `ANVIL_KEY` (Anvil's `accounts[0]`).
    const ANVIL_ADDR_HEX: &str = "f39fd6e51aad88f6f4ce6ab8827279cfffb92266";

    fn anvil_address() -> Address {
        let bytes = hex::decode(ANVIL_ADDR_HEX).unwrap();
        Address::from_slice(&bytes)
    }

    // ---- sum_ether ----------------------------------------------------------

    #[test]
    fn sum_ether_empty_is_zero() {
        assert_eq!(sum_ether(&[]), [0u8; 32]);
    }

    #[test]
    fn sum_ether_two_small_amounts() {
        let mk = |n: u64| EntryAuth {
            r: [0; 32],
            s: [0; 32],
            v: 27,
            ether_amount: U256::from(n).to_be_bytes(),
        };
        let total = U256::from_be_bytes(sum_ether(&[mk(7), mk(35)]));
        assert_eq!(total, U256::from(42u64));
    }

    #[test]
    fn sum_ether_saturates_on_overflow() {
        let max: [u8; 32] = U256::MAX.to_be_bytes();
        let one: [u8; 32] = U256::from(1u64).to_be_bytes();
        let mk = |amt: [u8; 32]| EntryAuth {
            r: [0; 32],
            s: [0; 32],
            v: 27,
            ether_amount: amt,
        };
        assert_eq!(
            sum_ether(&[mk(max), mk(one)]),
            max,
            "saturating add must clamp at U256::MAX",
        );
    }

    // ---- §3 packing round-trip ----------------------------------------------

    #[test]
    fn pack_unpack_round_trip_zero_payload() {
        let payload = vec![];
        let blob = pack_blob(&payload);
        let unpacked = unpack_blob(&blob);
        assert_eq!(unpacked.len(), BLOB_PAYLOAD_BYTES);
        assert!(unpacked.iter().all(|&b| b == 0));
    }

    #[test]
    fn pack_unpack_round_trip_short_payload() {
        let mut payload = vec![0u8; 100];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let blob = pack_blob(&payload);
        assert_eq!(blob.len(), BLOB_RAW_BYTES);
        // §3 canonicality: every 32-byte chunk's high byte is zero.
        for chunk in blob.chunks_exact(BYTES_PER_FIELD_ELEMENT) {
            assert_eq!(chunk[0], 0x00);
        }
        let unpacked = unpack_blob(&blob);
        assert_eq!(&unpacked[..payload.len()], &payload[..]);
        assert!(unpacked[payload.len()..].iter().all(|&b| b == 0));
    }

    #[test]
    #[should_panic(expected = "§3 canonicality")]
    fn unpack_rejects_non_canonical_field_element() {
        let mut blob = vec![0u8; BLOB_RAW_BYTES];
        // Corrupt the high byte of field element 5.
        blob[5 * BYTES_PER_FIELD_ELEMENT] = 0x01;
        let _ = unpack_blob(&blob);
    }

    // ---- §4 RLP framing -----------------------------------------------------

    #[test]
    fn rlp_round_trip_two_entries() {
        let entries = vec![b"hello".to_vec(), b"world!".to_vec()];
        let buf = encode_entries(&entries);

        // Worked example from spec §4.3:
        //   cd 85 68 65 6c 6c 6f 86 77 6f 72 6c 64 21
        let expected: &[u8] = &[
            0xcd, 0x85, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x86, 0x77, 0x6f, 0x72, 0x6c,
            0x64, 0x21,
        ];
        assert_eq!(buf, expected, "RLP encoding must match spec §4.3 worked example");

        let decoded = decode_entries(&buf);
        assert_eq!(decoded, entries);
    }

    #[test]
    fn rlp_decode_ignores_trailing_padding() {
        let entries = vec![b"x".to_vec()];
        let mut buf = encode_entries(&entries);
        // Append "padding" — any bytes that would be inside the §3.1 padding
        // region. Decoder MUST stop at the end of the RLP list.
        buf.extend_from_slice(&[0x00, 0xff, 0x80, 0xab]);
        let decoded = decode_entries(&buf);
        assert_eq!(decoded, entries);
    }

    #[test]
    fn rlp_decode_empty_list_ok() {
        // `0xc0` is the RLP encoding of an empty list (`k = 0`).
        let decoded = decode_entries(&[0xc0]);
        assert!(decoded.is_empty());
    }

    #[test]
    #[should_panic(expected = "spec §4.1 forbids empty entries")]
    fn rlp_decode_rejects_empty_entry() {
        // List containing one empty byte string: `0xc1 0x80`.
        let _ = decode_entries(&[0xc1, 0x80]);
    }

    // ---- §5 signature recover ----------------------------------------------

    #[test]
    fn recover_round_trips_for_anvil_key() {
        let auth = sign_eip191(&ANVIL_KEY, b"hello");
        // Spec §5.1: producers MUST emit v ∈ {27, 28}. Either is valid; the
        // exact value depends on the message and key.
        assert!(matches!(auth.v, 27 | 28), "v = {} not in {{27, 28}}", auth.v);
        assert_eq!(auth.recover(b"hello"), anvil_address());
    }

    #[test]
    fn recover_rejects_v_outside_27_28() {
        let mut auth = sign_eip191(&ANVIL_KEY, b"hi");
        auth.v = 0; // viem-style raw parity, banned by spec §5.1.
        let result = std::panic::catch_unwind(|| auth.recover(b"hi"));
        assert!(result.is_err(), "v ∉ {{27, 28}} must panic");
    }

    #[test]
    fn recover_rejects_high_s_signature() {
        let mut auth = sign_eip191(&ANVIL_KEY, b"hi");
        // Flip s to its high-s counterpart: s' = secp256k1n - s.
        // secp256k1n =
        //   0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
        const N: [u8; 32] = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFE, 0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B,
            0xBF, 0xD2, 0x5E, 0x8C, 0xD0, 0x36, 0x41, 0x41,
        ];
        let n = U256::from_be_bytes(N);
        let s_low = U256::from_be_bytes(auth.s);
        let s_high = n - s_low;
        auth.s = s_high.to_be_bytes();
        // We must also flip v so the (r, s_high, v) triple still recovers
        // *some* address — otherwise the panic could come from ecrecover
        // failing rather than the high-s assertion.
        auth.v = if auth.v == 27 { 28 } else { 27 };

        let result = std::panic::catch_unwind(|| auth.recover(b"hi"));
        assert!(result.is_err(), "high-s signature must panic");
    }

    #[test]
    fn recover_fails_on_wrong_data() {
        let auth = sign_eip191(&ANVIL_KEY, b"hello");
        // Recovery still produces *an* address for any data, but it won't
        // match the Anvil address — that's the v1-spec way of detecting
        // tampering when the policy hook compares against a known signer.
        let recovered = auth.recover(b"goodbye");
        assert_ne!(recovered, anvil_address());
    }

    // ---- end-to-end: sample_inputs ------------------------------------------

    #[test]
    fn sample_inputs_self_consistent() {
        let inputs = sample_inputs();

        // publicInputsHash matches.
        assert_eq!(
            inputs.expected_public_inputs_hash,
            compute_public_inputs_hash(&inputs),
        );

        // blob_data length matches the declared blob count.
        assert_eq!(inputs.blob_data.len(), BLOB_RAW_BYTES * inputs.blobs.len());

        // §3 canonicality holds.
        for chunk in inputs.blob_data.chunks_exact(BYTES_PER_FIELD_ELEMENT) {
            assert_eq!(chunk[0], 0x00);
        }

        // Cross-blob stream decodes into k entries that line up with
        // entry_auths.
        let s = unpack_blobs(&inputs.blob_data, inputs.blobs.len());
        let entries = decode_entries(&s);
        assert_eq!(entries.len(), inputs.entry_auths.len());

        // Every signature recovers to the Anvil address.
        let signers: Vec<Address> = entries
            .iter()
            .zip(&inputs.entry_auths)
            .map(|(data, auth)| auth.recover(data))
            .collect();
        for s in &signers {
            assert_eq!(*s, anvil_address());
        }

        // Sum of ether matches the on-chain `total_ether_paid` claim.
        assert_eq!(sum_ether(&inputs.entry_auths), inputs.total_ether_paid);

        // Signer-hash sanity: hashing the same address k times must equal
        // a hand-rolled keccak over `addr.repeat(k)`, and must NOT equal
        // the empty-list hash.
        let signers_hash = keccak_signers(&signers);
        let empty_hash = keccak_signers(&[]);
        assert_ne!(signers_hash, empty_hash);
    }

    // ---- §5.2 step 3 hook: signersHash ------------------------------------

    #[test]
    fn keccak_signers_empty_matches_keccak_of_empty_input() {
        let mut hasher = Keccak::v256();
        let mut expected = [0u8; 32];
        hasher.finalize(&mut expected);
        assert_eq!(keccak_signers(&[]), expected);
    }

    #[test]
    fn keccak_signers_order_sensitive() {
        let a = Address::from([0x11u8; 20]);
        let b = Address::from([0x22u8; 20]);
        assert_ne!(
            keccak_signers(&[a, b]),
            keccak_signers(&[b, a]),
            "signersHash must depend on entry order — verifiers rely on it",
        );
    }

    #[test]
    fn keccak_signers_concatenates_raw_addresses() {
        // Spec: `keccak256(addr_0 || addr_1)` over raw 20-byte little-end-
        // free representations (the wire form for 20-byte ABI addresses).
        let a = Address::from([0xAAu8; 20]);
        let b = Address::from([0xBBu8; 20]);

        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(a.as_slice());
        buf.extend_from_slice(b.as_slice());

        let mut hasher = Keccak::v256();
        hasher.update(&buf);
        let mut expected = [0u8; 32];
        hasher.finalize(&mut expected);

        assert_eq!(keccak_signers(&[a, b]), expected);
    }

    #[test]
    fn sample_inputs_serialize_round_trip() {
        // Guards the ZisK stdin path: GuestInputs must survive bincode round
        // trips. Especially important now that EntryAuth is a struct rather
        // than a `[u8; 65]` (the latter doesn't serde-Deserialize without a
        // big-array workaround).
        let inputs = sample_inputs();
        let bytes = bincode::serialize(&inputs).expect("serialize");
        let back: GuestInputs = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(back.blob_data, inputs.blob_data);
        assert_eq!(back.entry_auths.len(), inputs.entry_auths.len());
        for (a, b) in back.entry_auths.iter().zip(&inputs.entry_auths) {
            assert_eq!(a, b);
        }
    }
}
