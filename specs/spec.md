# BlobShare — Blob Encoding Specification

Encoding format for application-level data carried inside an EIP-4844 blob, plus
the out-of-band Ethereum signature scheme used to authenticate each entry.

## 1. Background

EIP-4844 (proto-danksharding) introduces "blob"-carrying transactions
(type `0x03`). Each blob is exactly **4096 field elements** of **32 bytes**
each, for a raw size of `131072` bytes (128 KiB). Every field element MUST be
a canonical scalar of the BLS12-381 scalar field, i.e. strictly less than:

```
BLS_MODULUS = 52435875175126190479447740508185965837690552500527637822603658699938581184513
```

`BLS_MODULUS` is `~ 2^254.857`, so a full 32-byte little/big-endian value
will frequently exceed the modulus. To guarantee canonicality with no
modular reduction, this spec uses only **31 bytes** of every field element
and reserves the most significant byte as `0x00`. This is the same pattern
used by every public EIP-4844 data-availability layer in production.

## 2. Constants

| Name                          | Value                                                              |
| ----------------------------- | ------------------------------------------------------------------ |
| `FIELD_ELEMENTS_PER_BLOB`     | `4096`                                                             |
| `BYTES_PER_FIELD_ELEMENT`     | `32`                                                               |
| `USABLE_BYTES_PER_FIELD`      | `31`                                                               |
| `BLOB_RAW_BYTES`              | `4096 * 32 = 131072`                                               |
| `BLOB_PAYLOAD_BYTES`          | `4096 * 31 = 126976`                                               |
| `BLS_MODULUS`                 | see §1                                                             |

## 3. Field-Element Packing (32 ↔ 31)

The blob is treated as the concatenation of 4096 field elements
`F[0] .. F[4095]`. Each `F[i]` is 32 bytes. Reserve byte 0 (the most
significant byte, big-endian) as a zero pad, and use bytes 1..31 as
payload:

```
F[i]  =  0x00 || P[i]                       (32 bytes total)
P[i]  =  F[i][1..32]                        (31 bytes payload)
```

Define the **per-blob payload** `P_blob` as the concatenation of one
blob's field-element payloads:

```
P_blob  =  P[0] || P[1] || ... || P[4095]
|P_blob| =  126976 bytes  (= BLOB_PAYLOAD_BYTES)
```

### 3.0 Logical Stream Across a Transaction

A type-3 transaction MAY carry up to 6 blobs. Let `B_0, B_1, …,
B_{n-1}` be the blobs of a single transaction in the order they
appear in the transaction's `blobVersionedHashes` field, and let
`P_j` be each blob's payload as decoded by §3.2. The transaction's
**logical stream** `S` is the concatenation of all per-blob
payloads:

```
S    =  P_0 || P_1 || ... || P_{n-1}
|S|  =  n * BLOB_PAYLOAD_BYTES
```

The framing rules in §4 apply to `S` as a whole. Entries MAY straddle
blob boundaries; verifiers reconstructing entries MUST fetch all `n`
blobs of the producing transaction.

### 3.1 Encoder (payload bytes → blob)

```
def encode_blob(payload: bytes) -> bytes:
    assert len(payload) <= BLOB_PAYLOAD_BYTES
    # Padding (the bytes past `len(payload)` up to BLOB_PAYLOAD_BYTES)
    # is any byte sequence that preserves §3 canonicality. Zero-padding
    # (shown) is one acceptable choice; viem's `toBlobs` writes a 0x80
    # sentinel followed by zeros and is also acceptable.
    payload = payload + b"\x00" * (BLOB_PAYLOAD_BYTES - len(payload))
    blob = bytearray()
    for i in range(FIELD_ELEMENTS_PER_BLOB):
        chunk = payload[i*31 : (i+1)*31]
        blob.append(0x00)            # high byte: zero pad
        blob.extend(chunk)           # 31 payload bytes
    assert len(blob) == BLOB_RAW_BYTES
    return bytes(blob)
```

### 3.2 Decoder (blob → payload bytes)

```
def decode_blob(blob: bytes) -> bytes:
    assert len(blob) == BLOB_RAW_BYTES
    payload = bytearray()
    for i in range(FIELD_ELEMENTS_PER_BLOB):
        fe = blob[i*32 : (i+1)*32]
        assert fe[0] == 0x00         # MUST be canonical
        payload.extend(fe[1:32])
    assert len(payload) == BLOB_PAYLOAD_BYTES
    return bytes(payload)
```

A blob whose any field element has `F[i][0] != 0x00` is **invalid** under
this spec and MUST be rejected.

## 4. Logical Payload Layout (RLP-Framed Entries)

The transaction's logical stream `S` (§3.0) carries an RLP-encoded list
of entries, followed by §3.1 padding:

```
S  =  RLP([ data_0, data_1, ..., data_{k-1} ])  ||  padding
```

Where:

* `RLP(...)` is the canonical Recursive Length Prefix encoding as
  defined by the Ethereum Yellow Paper, Appendix B.
* Each `data_i` is opaque application data, encoded as an RLP byte
  string.
* After the RLP list, the remainder of `S` is padding (§3.1).

### 4.1 Constraints

* `len(RLP_bytes) <= |S| = n * BLOB_PAYLOAD_BYTES`, where `n` is the
  number of blobs in the producing transaction.
* The RLP encoding MUST be canonical: minimal-length integer prefixes,
  no trailing junk inside the list body. Decoders MUST reject
  non-canonical RLP.
* RLP self-delimits each item, so there is no separate terminator and
  no per-entry length cap beyond `BLOB_PAYLOAD_BYTES * n`.

### 4.2 Parser

```
def parse_entries(S: bytes) -> list[bytes]:
    assert len(S) % BLOB_PAYLOAD_BYTES == 0
    items, _consumed = rlp_decode_list(S)   # canonical RLP only
    return items                            # padding bytes ignored
```

`rlp_decode_list` is the standard canonical RLP list decoder
(Yellow Paper Appendix B). It reads exactly the bytes covered by the
list's length prefix; any bytes after that are §3.1 padding and MUST
NOT be interpreted.

### 4.3 Worked Example

Two entries `data_0 = "hello"` (5 bytes) and `data_1 = "world!"` (6
bytes). Canonical RLP encodings of each item:

```
RLP("hello")   =  85 68 65 6c 6c 6f                  # 6 bytes
RLP("world!")  =  86 77 6f 72 6c 64 21               # 7 bytes
```

The list's payload length is `6 + 7 = 13` bytes, so the list prefix is
`0xc0 + 13 = 0xcd`:

```
RLP([...])     =  cd 85 68 65 6c 6c 6f 86 77 6f 72 6c 64 21   # 14 bytes
```

Therefore:

```
S[0..14]   =  cd 85 68 65 6c 6c 6f 86 77 6f 72 6c 64 21
S[14..]    =  padding (§3.1)
```

After 31→32 packing (§3.1), `S` becomes the on-chain blob (or
sequence of blobs, per §3.0).

### 4.4 Reference Implementations (informative)

The amended framing maps directly onto viem's blob and RLP helpers:

```ts
import { fromBlobs, fromRlp, toBlobs, toBytes, toRlp } from 'viem'

// Encoder
const data    = toRlp(payloads.map(toBytes))   // §4
const blobs   = toBlobs({ data })              // §3.1, multi-blob via §3.0

// Decoder (given every blob of the tx in order)
const stitched  = fromBlobs({ blobs, to: 'hex' })
const recovered = fromRlp(stitched, 'bytes')
```

Equivalent helpers exist in ethers.js, `@ethereumjs/rlp`, and the
Python `rlp` and `eth-rlp` packages; any canonical RLP library is
acceptable.

## 5. Per-Entry Ethereum Signature (Out-of-Band)

Each `data_i` is independently signed by its author. **Signatures are NOT
included in the blob.** They are distributed alongside the blob (e.g. in a
sidecar JSON, an HTTP response, or a smart-contract call) and bound to a
specific blob via the blob's versioned hash.

### 5.1 Signing

The signer follows EIP-191 `personal_sign` (the same scheme used by
`eth_sign` in MetaMask, viem, ethers, etc.):

```
prefix  = "\x19Ethereum Signed Message:\n" || ascii(len(data_i))
digest  = keccak256( prefix || data_i )
sig_i   = secp256k1_sign(privkey, digest)        # 65 bytes: r || s || v
```

Note: only `data_i` is signed. The surrounding RLP framing (§4) and
any blob-level packing (§3) are NOT covered by the signature — they
are recoverable from the blob(s) themselves.

Reference (informative): viem's `hashMessage(data)` is the canonical
implementation of the digest above; viem's
`verifyMessage({ address, message, signature })` is the canonical
verifier. Equivalent helpers exist in ethers.js, web3.py, etc.

### 5.2 Verification

A verifier:

1. Fetches every blob of the producing transaction (or their
   KZG-committed contents), decodes each per §3.2, concatenates them
   per §3.0 to reconstruct `S`, and runs §4.2 on `S` to obtain
   `data_0 .. data_{k-1}`.
2. For each `(data_i, sig_i)` pair received out-of-band, recomputes
   `digest_i` per §5.1 and runs `ecrecover(digest_i, sig_i)` to obtain
   the signer address.
3. Accepts the entry iff the recovered address matches the expected
   author (e.g. an allow-list, an on-chain registry, or a value bound
   to one of the transaction's `blobVersionedHashes` on L1).

### 5.3 Sidecar Format (informative)

A minimal sidecar pairs each entry with its signature by index:

```json
{
  "blobVersionedHash": "0x01...",
  "entries": [
    { "index": 0, "signer": "0xAbC...", "sig": "0x..." },
    { "index": 1, "signer": "0xDeF...", "sig": "0x..." }
  ]
}
```

The sidecar is non-normative; any transport that delivers `(i, sig_i)`
pairs is acceptable.

## 6. Security Notes

* **Canonicality.** The `F[i][0] == 0x00` invariant is the only thing
  keeping every field element below `BLS_MODULUS`. Encoders MUST NOT
  write a non-zero high byte; decoders MUST reject blobs that do.
  This rule applies to padding bytes (§3.1) as well as to bytes
  covered by the RLP framing.
* **No framing authentication.** Because only `data_i` is signed, a
  malicious republisher can re-frame the same `data_i` into a
  different RLP list, a different blob layout, or a different
  transaction. Consumers that care about ordering or co-location MUST
  bind to `blobVersionedHash` (e.g. by including it in whatever they
  hash on L1).
* **Replay across blobs.** A signature over `data_i` is valid for any
  blob that contains the exact same `data_i`. If replay matters,
  include a nonce or `blobVersionedHash` inside `data_i`.
* **Cross-blob assembly.** A verifier MUST fetch every blob of the
  producing transaction (by `txHash` or by the full
  `blobVersionedHashes` set) before decoding entries. An entry MAY
  straddle blob boundaries; consumers holding a strict subset of a
  tx's blobs cannot recover such entries.

## 7. References

* EIP-4844 — Shard Blob Transactions:
  <https://eips.ethereum.org/EIPS/eip-4844>
* Consensus specs, `deneb/polynomial-commitments.md` (defines
  `FIELD_ELEMENTS_PER_BLOB`, `BLS_MODULUS`):
  <https://github.com/ethereum/consensus-specs/blob/dev/specs/deneb/polynomial-commitments.md>
* EIP-191 — Signed Data Standard:
  <https://eips.ethereum.org/EIPS/eip-191>
* Ethereum Yellow Paper, Appendix B (Recursive Length Prefix):
  <https://ethereum.github.io/yellowpaper/paper.pdf>
* viem — informative reference implementation of `toBlobs`,
  `fromBlobs`, `toRlp`, `fromRlp`, `hashMessage`, `verifyMessage`:
  <https://viem.sh/>
