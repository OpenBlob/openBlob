# BlobShare — Proposed Amendments for viem-Native Producers

Three proposed changes to [spec.md](./spec.md) that collapse the encoder
side to a few off-the-shelf [viem](https://viem.sh) calls without
weakening the on-chain data-availability guarantees. Each change is
independent; they compose, but any subset can be adopted.

## Summary

| # | Change                                                       | Spec section affected | Encoder effect                          |
| - | ------------------------------------------------------------ | --------------------- | --------------------------------------- |
| 1 | Replace length-prefix framing with RLP                       | §4                    | `toRlp(payloads)` instead of manual u32 |
| 2 | Define `S` across all blobs in a tx, not per blob            | §4                    | `toBlobs(data)` chunks for free         |
| 3 | Allow non-zero padding after the framing terminator          | §3.1, §4              | `toBlobs` `0x80` terminator becomes legal |

With all three adopted, a conformant publisher reduces to:

```ts
const data = toRlp(payloads.map(toBytes))
const blobs = toBlobs({ data })
const txHash = await client.sendTransaction({ blobs, kzg, to, maxFeePerBlobGas })
```

The byte-level rules in §3 stay authoritative for non-viem implementers;
viem just stops needing a custom adapter.

---

## Amendment 1 — Use RLP instead of length-prefixed entries

### Motivation

§4 currently frames the logical stream `S` as

```
S = ( len_0 || data_0 ) || ( len_1 || data_1 ) || ... || 0x00000000 || padding
```

with each `len_i` a 4-byte big-endian `uint32` and `0x00000000` as the
end-of-entries terminator. That requires every implementer to hand-write
a fixed-width length codec and a sentinel-driven parser, and it
re-introduces §6's "trailing-zero ambiguity" footnote. RLP solves all
three problems with a single primitive that every EVM ecosystem already
ships.

### Proposed text (replaces §4)

> The 126976-byte logical stream `S` carries an RLP-encoded list of
> entries, followed by zero padding to `BLOB_PAYLOAD_BYTES`:
>
> ```
> S  =  RLP([data_0, data_1, ..., data_{k-1}])  ||  padding
> ```
>
> Where:
>
> * `RLP(...)` is the canonical Recursive Length Prefix encoding as
>   defined by the Ethereum Yellow Paper, Appendix B.
> * Each `data_i` is opaque application data, encoded as an RLP byte
>   string.
> * After the RLP payload, the remainder of `S` is padding (see §3.1).
>
> #### Constraints
>
> * `len_RLP <= BLOB_PAYLOAD_BYTES` for the entire encoded list.
> * The RLP list MUST be canonical: minimal-length integer prefixes,
>   no trailing junk inside the list body. Decoders MUST reject
>   non-canonical RLP.
>
> #### Parser
>
> ```python
> def parse_entries(S: bytes) -> list[bytes]:
>     assert len(S) == BLOB_PAYLOAD_BYTES
>     entries, _consumed = rlp_decode_list(S)  # canonical RLP
>     return entries
> ```

### Reference encoder/decoder (informative)

```ts
import { toRlp, fromRlp, toBytes } from 'viem'
const rlpHex   = toRlp(payloads.map(toBytes))         // encode
const payloads = fromRlp(rlpHex, 'bytes')             // decode
```

### Trade-offs

* Removes the `0x00000000` terminator and the §6 "trailing-zero
  ambiguity" caveat — RLP is self-delimiting per item.
* Removes the artificial `len_i <= 2^32 - 1` cap; the only remaining
  limit is `BLOB_PAYLOAD_BYTES`.
* Adds an RLP dependency. Mitigation: every EVM-adjacent language already
  has a tested RLP library; spec readers do not need to re-implement.
* Slightly larger framing overhead than 4 bytes per entry for very
  short entries (RLP uses 1–9 bytes of header). Negligible at the
  payload sizes this spec targets.

---

## Amendment 2 — Treat `S` as the concatenation across all blobs in the tx

### Motivation

§4 currently treats each blob as an independent container: every blob
in a multi-blob transaction has its own `len_i || data_i || ...`
sequence with its own terminator. That works, but it forces an encoder
to make blob-boundary decisions and re-emit a fresh terminator per
blob. viem's `toBlobs(data)` instead chunks a single byte stream across
however many blobs are needed, with no per-blob framing.

Aligning the spec with that semantics makes `toBlobs` the canonical
encoder for free.

### Proposed text (replaces the per-blob framing language in §3.1 / §4)

> Let `B_0, B_1, ..., B_{n-1}` be the blobs of a single type-3
> transaction in the order they appear in the transaction's
> `blobVersionedHashes` field. For each blob `B_j`, let `P_j` be its
> 126976-byte payload as decoded by §3.2. The logical stream `S` of
> the transaction is the concatenation
>
> ```
> S  =  P_0 || P_1 || ... || P_{n-1}
> |S|  =  n * BLOB_PAYLOAD_BYTES
> ```
>
> The framing rules in §4 apply to `S` as a whole. Entries MAY straddle
> blob boundaries; verifiers reconstructing entries MUST fetch all `n`
> blobs of the transaction.

### Reference encoder (informative)

```ts
const blobs = toBlobs({ data: rlpHex })   // one or more blobs, auto-chunked
```

### Trade-offs

* A verifier holding only a subset of a transaction's blobs cannot
  decode entries that straddle a missing blob. This matches how every
  modern blob consumer (rollups, blob-indexers, DA layers) already
  operates: they fetch all blobs of a tx by `txHash` or by the full
  `blobVersionedHashes` set.
* Per-blob signers / sidecar formats (§5.3) continue to bind via
  `blobVersionedHash` of any one blob the entry appears in; if an
  entry straddles, sidecars MAY list multiple versioned hashes.
* Producers that want one-blob-per-entry semantics can still emit one
  blob per RLP list and one tx per blob — Amendment 2 widens the
  spec, it does not narrow it.

---

## Amendment 3 — Allow non-zero padding after the framing terminator

### Motivation

§3.1 says everything after the last logical byte of `S` is `0x00`
padding. viem's `toBlobs`, after the last data byte, writes a single
`0x80` byte (an end-of-data marker for its `fromBlobs` partner) and
then zeros. The byte is harmless: it sits past the framing terminator,
inside the §3.1 "padding" region, and a §4 parser stops before
reaching it. But by the literal text of §3.1 it is non-conformant.

A one-sentence relaxation legalizes the existing viem output without
weakening canonicality (the only invariant that actually matters for
KZG validity).

### Proposed text (amends §3.1, after the encoder pseudo-code)

> Bytes of `S` past the last byte covered by §4's framing are
> *padding*. Padding bytes MUST preserve §3 canonicality
> (`F[i][0] == 0x00` for every field element they occupy) but are
> otherwise unconstrained. Decoders MUST NOT interpret padding bytes
> and MUST treat the framing terminator from §4 as the sole signal of
> end-of-entries.

### Trade-offs

* Canonicality (the §3 high-byte invariant that keeps every field
  element below `BLS_MODULUS`) is unchanged — this is the only rule
  KZG actually depends on.
* §6's "Trailing-zero ambiguity" note becomes redundant once Amendment
  1 lands (RLP is self-delimiting); under Amendment 3 alone, the §4
  `0x00000000` terminator still resolves ambiguity for length-prefix
  decoders.
* Producers who *want* strict zero padding (e.g. for deterministic
  blob hashes across encoders) MAY still emit it — the relaxation is
  one-sided.

---

## Combined effect on the reference publisher

Adopting all three amendments, the entire bundler-side encode path is:

```ts
import { toBlobs, toBytes, toRlp, parseGwei } from 'viem'

const data    = toRlp(microblobs.map((m) => toBytes(m.payload)))
const blobs   = toBlobs({ data })
const kzg     = await getKzg()             // viem-shaped Kzg
const txHash  = await client.sendTransaction({
  blobs,
  kzg,
  to,
  maxFeePerBlobGas: parseGwei('30'),
})
```

The verifier side, given a tx hash:

```ts
import { fromBlobs, fromRlp } from 'viem'

const blobs    = await fetchBlobs(txHash)             // beacon-chain API
const data     = fromBlobs({ blobs, to: 'hex' })      // re-stitch S
const payloads = fromRlp(data, 'bytes')               // §4'
// then for each (payload_i, sig_i) pair from the sidecar:
//   verifyMessage({ address, message: payload_i, signature: sig_i })
```

No custom field-element packer, no manual length codec, no terminator
sentinel logic. The byte-level rules in §3 still govern what's on
chain; viem just stops being a second-class citizen.
