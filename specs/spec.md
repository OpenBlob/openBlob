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
| `LENGTH_PREFIX_BYTES`         | `4` (big-endian `uint32`)                                          |
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

Define the **logical payload stream** `S` as the concatenation:

```
S  =  P[0] || P[1] || ... || P[4095]
|S| =  126976 bytes
```

### 3.1 Encoder (payload bytes → blob)

```
def encode_blob(payload: bytes) -> bytes:
    assert len(payload) <= BLOB_PAYLOAD_BYTES
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

## 4. Logical Payload Layout (Length-Prefixed Entries)

The 126976-byte logical stream `S` carries a sequence of entries, each
prefixed by its length:

```
S  =  ( len_0 || data_0 ) || ( len_1 || data_1 ) || ... || padding
```

Where:

* `len_i` is a **4-byte big-endian unsigned integer** giving the length of
  `data_i` in bytes.
* `data_i` is `len_i` bytes of opaque application data.
* After the last entry, the remainder of `S` is zero-padded to
  `BLOB_PAYLOAD_BYTES`.
* A length prefix of `0x00000000` (i.e. `len = 0`) signals **end of
  entries**; everything after it is padding and MUST NOT be parsed.

### 4.1 Constraints

* `1 <= len_i <= 2^32 - 1` for every real entry.
* `sum_i (4 + len_i) <= BLOB_PAYLOAD_BYTES = 126976`.
* An entry MUST NOT straddle the end of `S`. The decoder MUST reject a
  blob whose final entry would extend past byte `126976`.

### 4.2 Parser

```
def parse_entries(S: bytes) -> list[bytes]:
    assert len(S) == BLOB_PAYLOAD_BYTES
    entries, cur = [], 0
    while cur + LENGTH_PREFIX_BYTES <= len(S):
        n = int.from_bytes(S[cur:cur+4], "big")
        cur += 4
        if n == 0:
            break                     # terminator / start of padding
        if cur + n > len(S):
            raise ValueError("entry overruns blob")
        entries.append(S[cur:cur+n])
        cur += n
    return entries
```

### 4.3 Worked Example

Two entries `data_0 = "hello"` (5 bytes) and `data_1 = "world!"` (6
bytes):

```
S[0..4]    = 00 00 00 05                 # len_0 = 5
S[4..9]    = 68 65 6c 6c 6f              # "hello"
S[9..13]   = 00 00 00 06                 # len_1 = 6
S[13..19]  = 77 6f 72 6c 64 21           # "world!"
S[19..23]  = 00 00 00 00                 # terminator
S[23..]    = 00 00 ... 00                # zero padding
```

After 31→32 packing (§3.1), `S` becomes the on-chain blob.

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

Note: only `data_i` is signed. The 4-byte `len_i` prefix and any
surrounding blob framing are NOT covered by the signature — they are
recoverable from the blob itself.

### 5.2 Verification

A verifier:

1. Fetches the blob (or its KZG-committed contents) and decodes it per
   §3.2 and §4.2 to obtain `data_0 .. data_{k-1}`.
2. For each `(data_i, sig_i)` pair received out-of-band, recomputes
   `digest_i` per §5.1 and runs `ecrecover(digest_i, sig_i)` to obtain
   the signer address.
3. Accepts the entry iff the recovered address matches the expected
   author (e.g. an allow-list, an on-chain registry, or a value bound to
   the blob's versioned hash on L1).

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
* **No length-prefix authentication.** Because only `data_i` is signed,
  a malicious republisher can re-frame the same `data_i` into a
  different blob layout. Consumers that care about ordering or
  co-location MUST bind to `blobVersionedHash` (e.g. by including it in
  whatever they hash on L1).
* **Replay across blobs.** A signature over `data_i` is valid for any
  blob that contains the exact same `data_i`. If replay matters, include
  a nonce or `blobVersionedHash` inside `data_i`.
* **Trailing-zero ambiguity.** `data_i` MAY contain trailing `0x00`
  bytes; the length prefix disambiguates them from padding. The `len =
  0` terminator is therefore not optional — without it, an all-zero tail
  could be mistaken for a giant zero-filled entry.

## 7. References

* EIP-4844 — Shard Blob Transactions:
  <https://eips.ethereum.org/EIPS/eip-4844>
* Consensus specs, `deneb/polynomial-commitments.md` (defines
  `FIELD_ELEMENTS_PER_BLOB`, `BLS_MODULUS`):
  <https://github.com/ethereum/consensus-specs/blob/dev/specs/deneb/polynomial-commitments.md>
* EIP-191 — Signed Data Standard:
  <https://eips.ethereum.org/EIPS/eip-191>
