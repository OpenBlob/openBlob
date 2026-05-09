# OpenBlob ZisK Proof

A ZisK zkVM circuit that mirrors `OpenBlob.proofBlobDA`'s on-chain digest:

```
publicInputsHash = keccak256(abi.encode(
    blobhashes, hashedData, prevRoot, newRoot, totalEtherPaid, blockHash
))
```

The guest commits `Output { publicInputsHash, valid }`. `valid` is currently a stub (`true`); the real state-transition checks come later.

## Layout

```
ziskProgram/
├── Cargo.toml          # workspace root (patches tiny-keccak → zisk syscall_keccak_f)
├── common/             # shared types: GuestInputs, Output, sample_inputs()
├── guest/              # the zkVM program (target: riscv64ima-zisk-zkvm-elf)
└── host/
    ├── src/main.rs     # default bin: full upload → setup → execute → prove → verify
    └── bin/
        ├── run.rs      # emulator only (with profiling)
        ├── execute.rs  # prover client execute, no proof
        ├── prove.rs    # generate + verify VADCOP proof, save/reload
        ├── minimal.rs  # VADCOP → final-minimal proof
        └── plonk.rs    # wrap as PLONK SNARK
```

## Prerequisites

- ZisK toolchain on `PATH` (`cargo-zisk`, `ziskemu`). If missing, install with `ziskup` — see the [ZisK install docs](https://0xpolygonhermez.github.io/zisk/getting_started/installation.html).
- Proving keys under `~/.zisk/provingKey` (downloaded by `ziskup`; required for the prove/verify paths).
- Rust toolchain (stable + the `riscv64ima-zisk-zkvm-elf` target installed by `ziskup`).

## Build

```bash
cd ziskProgram
cargo build --release
```

The guest ELF is built automatically by the host's `build.rs`.

## Running

All commands are run from `ziskProgram/`.

### 1. Emulator only (fastest, no keys needed)

```bash
cargo run --release --bin run
```

Runs the guest in `ziskemu` with profiling, prints cycles. Good for iterating on the guest.

### 2. Execute via prover client (no proof)

```bash
cargo run --release --bin execute
```

Runs through the prover client's execute path, then decodes and prints the committed `Output` (`publicInputsHash`, `valid`).

### 3. Full prove + verify (default `host` bin — `src/main.rs`)

```bash
cargo run --release -p host
```

End-to-end: upload program → setup → execute → generate VADCOP proof → verify against the program VK. Heavy: several minutes and significant RAM.

### 4. Prove with disk round-trip

```bash
cargo run --release --bin prove
```

Same as above, plus saves the proof to `tmp/openblob_proof.bin`, reloads it, and re-verifies with the natively-computed `publicInputsHash`.

### 5. Minimal (VADCOP → final-minimal)

```bash
cargo run --release --bin minimal
```

Generates a VADCOP proof and reduces it to the final-minimal form.

### 6. PLONK SNARK wrapping

```bash
cargo run --release --bin plonk
```

Wraps the proof as a PLONK SNARK. Saves to `/tmp/openblob_proof_snark.bin`.

## Inputs

All binaries use `common::sample_inputs()` — a hardcoded batch (two blobhashes, zero `prevRoot`, etc.). Edit `common/src/lib.rs` to change the batch the proof is generated over.

## Notes

- `[patch.crates-io]` for `tiny-keccak` lives in the **workspace** `Cargo.toml`. Moving it into `guest/Cargo.toml` silently no-ops, so Keccak-f would fall back to the software round function and proving cost would explode.
- The committed `publicInputsHash` is designed to match `OpenBlob.proofBlobDA`'s on-chain `keccak256(abi.encode(...))` — the verifier compares them to bind the proof to a specific batch.
