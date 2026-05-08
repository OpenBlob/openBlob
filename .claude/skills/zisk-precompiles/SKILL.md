---
name: zisk-precompiles
description: Use ZisK precompiles — built-in zkVM syscalls that accelerate hashing (Keccak-f, SHA-256, Blake2b, Poseidon2), elliptic-curve operations (Secp256k1, Secp256r1, BN254, BLS12-381), and big-int arithmetic (Add256, Arith256, Arith384_mod). Use whenever the user wants their guest program to run faster, asks why their proof is so expensive, mentions patching `tiny-keccak` / `sha2` / `k256` / `secp256k1` / similar crates, asks how to "use a precompile in ZisK", wants to know which precompiles exist, or has profiling output showing a hash/curve op as the dominant cost. Trigger even if the user only says "speed up my Keccak", "my proof has too many cycles", or "how do I avoid recomputing SHA inside the zkVM".
---

# ZisK Precompiles

Precompiles are syscalls baked into the ZisK zkVM that compute expensive operations (hashing, EC ops, big-int math) in a single step instead of thousands. Using them is the most impactful optimization a guest program can make — the difference between a Keccak running for ~13M cycles and ~30K cycles is one of these calls.

The user almost never calls a precompile directly. Instead, they patch a third-party crate (e.g., `tiny-keccak`, `sha2`, `k256`) so that crate's ops dispatch to the ZisK syscall. This skill covers which precompiles exist, the patching pattern, and how to confirm the patches actually fired.

For the broader build/run flow, see **zisk-tooling**. For writing a guest from scratch, see **zisk-new-program**.

---

## Available precompiles

All FFI-callable as `extern "C"` functions from the `ziskos` crate. Source: `ziskos/entrypoint/src/syscalls/` in this repo.

### Hashes / compression
| Syscall | What |
|---|---|
| `syscall_keccak_f` | Keccak-f 1600 permutation |
| `syscall_sha256_f` | SHA-256 extend + compress |
| `syscall_blake2br` | BLAKE2b round function |
| `syscall_poseidon2` | Poseidon2 compression |

### Big-int arithmetic
| Syscall | What |
|---|---|
| `syscall_add256` | 256-bit unsigned addition |
| `syscall_arith256` | 256-bit `a × b + c` |
| `syscall_arith256_mod` | 256-bit `(a × b + c) mod n` |
| `syscall_arith384_mod` | 384-bit `(a × b + c) mod n` |

### Elliptic curves
| Syscall | What |
|---|---|
| `syscall_secp256k1_add` / `_dbl` | Bitcoin curve point add / double |
| `syscall_secp256r1_add` / `_dbl` | NIST P-256 point add / double |
| `syscall_bn254_curve_add` / `_dbl` | BN254 G1 add / double |
| `syscall_bn254_complex_add` / `_sub` / `_mul` | Quadratic-extension Fp2 ops over BN254 |
| `syscall_bls12_381_curve_add` / `_dbl` | BLS12-381 G1 add / double |
| `syscall_bls12_381_complex_add` / `_sub` / `_mul` | Quadratic-extension Fp2 ops over BLS12-381 |

If something is missing (e.g., G2 ops, pairings), the user should check the upstream `ziskos/entrypoint/src/syscalls/` directory before assuming it doesn't exist — new precompiles get added regularly.

---

## How to use precompiles: patch a crate

Calling syscalls by hand is rarely the right move. The idiomatic pattern is to **patch a third-party crate** that already implements the operation, swap its hot path for the syscall, and let everyone else's code that depends on that crate get the speedup for free.

ZisK ships a handful of pre-patched crates. The canonical example:

- `tiny-keccak` patched: <https://github.com/0xPolygonHermez/zisk-patch-tiny-keccak/tree/zisk>

To use a patched crate, add a `[patch.crates-io]` section to the **workspace root `Cargo.toml`** (the one above `guest/` and `host/`):

```toml
[patch.crates-io]
tiny-keccak = { git = "https://github.com/0xPolygonHermez/zisk-patch-tiny-keccak", branch = "zisk" }
```

Then rebuild. Any crate in the dependency tree that uses `tiny-keccak` now goes through the patched version, which dispatches Keccak-f calls to `syscall_keccak_f`.

**Where to put the patch — `[patch.crates-io]` only takes effect at the *workspace root*. Putting it inside `guest/Cargo.toml` does nothing.** This is the most common mistake.

### Finding existing patches

Before writing a new patch, search:

```
github.com/0xPolygonHermez/zisk-patch-<crate-name>
```

Patches typically live on a `zisk` branch of a fork. If a patch exists, use it. If not, see "Writing your own patch" below.

---

## Writing your own patch

The pattern, using SHA-256 as an example. Inside the crate's hash function, replace the round function with the ZisK syscall when compiled for the ZisK target:

```rust
// in your fork of the sha2 crate (or wherever the inner round runs):

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    #[cfg(target_os = "zkvm")]
    {
        // Marshal `state` and `block` into the layout the syscall expects,
        // then call into the ziskos FFI:
        unsafe { ziskos::syscalls::syscall_sha256_f(state_ptr, block_ptr); }
        return;
    }

    // Native (non-zkVM) implementation — runs unchanged on x86_64/macOS.
    native_compress(state, block);
}
```

Key rules:

1. **Gate on `target_os = "zkvm"`** (or whatever cfg the upstream crate already uses for this branching) so native builds keep the original implementation. Hint generation also needs this — see **zisk-hints** if you're touching hints.
2. **Match the syscall's expected memory layout exactly.** Each syscall in `ziskos/entrypoint/src/syscalls/` documents pointer alignment, byte order, and limb sizes. A subtle mismatch (e.g., big-endian vs. little-endian limbs in 256-bit big-int ops) will pass `verify-constraints` but produce wrong output.
3. **Don't alter call counts.** If the original `compress` runs once per 512-bit block, the patched one must too. The prover's planner sizes its precompile instances based on call counts; surprise extra calls can blow you across an instance boundary and double the cost (see profiling guide).
4. **Keep the public API identical.** Downstream crates expect a specific signature. Patches are drop-in replacements, not redesigns.

Once patched, point the workspace at your fork:

```toml
[patch.crates-io]
sha2 = { git = "https://github.com/your-username/sha2-zisk", branch = "zisk" }
```

---

## Confirm the patch actually fires

Patches are silent. A patch that compiles but doesn't dispatch (cfg gate wrong, wrong code path) will keep the program correct but slow — and the user won't notice until they look at numbers.

**Profile the guest with `ziskemu -X -S`** and check the opcode breakdown. The relevant precompile op (`keccak`, `sha256`, `secp256k1_add`, etc.) should appear with a non-trivial count. If the count is zero, the patch isn't firing.

```bash
ziskemu -e target/elf/riscv64ima-zisk-zkvm-elf/release/guest -i input.bin -X -S
```

In the **COST BY OPCODE** table:

```
OP keccak                32,650   2,466,707,500  21.57% #1
```

Shows Keccak ran 32,650 times via the precompile. If you instead see thousands of calls to a software-implementation function (`tiny_keccak::keccakf::...`) in **TOP COST FUNCTIONS** with no precompile op count, the patch isn't applied.

Filter for hash / curve namespaces with `--roi-filter` to spot unpatched paths:

```bash
ziskemu -e <elf> -i <input> -X -S --roi-filter "keccak|sha|secp"
```

---

## Cost-instance gotcha

ZisK precompiles run on dedicated state-machine **instances** with fixed capacity. Cost is per-instance, not per-call:

- 1 Keccak call → 1 Keccak instance
- 5,242 Keccak calls → still 1 instance (assuming capacity = 5,242)
- 5,243 Keccak calls → 2 instances → roughly **double** the cost for one extra call

This means using a precompile a tiny number of times has the same final cost as using it many times within one instance's capacity. **Profiling cost** scales linearly per call (this is what you optimize against); **final cost** jumps at instance boundaries.

The practical advice: when patching, you're nearly always making things faster, but watch for the case where adding a small number of additional calls crosses a boundary. A single extra Keccak call can look pathological in final cost.

See `book/developer/profiling.md` for the full breakdown of profiling vs. final cost.

---

## Common questions

**Why isn't there a precompile for X?** Either it isn't worth the engineering (rarely-used op) or it's planned. Check `ziskos/entrypoint/src/syscalls/` for the current list. Custom precompile-style operations can also be implemented as **custom hint handlers** — see **zisk-hints**.

**Can I write a precompile myself?** Not in user code. Precompiles are part of the prover circuit. What user code *can* add is custom **hints** (computed outside the zkVM, verified inside) — that's a different mechanism.

**Why doesn't `cargo build` complain about the `ziskos::syscalls` import?** Because `ziskos` provides a native fallback. The syscall is a thin wrapper that delegates to a software implementation when not building for the zkVM target. Patches benefit from this — same code, different backends.

**My patch makes the proof slower than the unpatched version.** Almost always means the patch is firing twice (once for the precompile, once for the original implementation as a fallback) or the `cfg` gate is too narrow / too wide. Re-read the gate; profile with `--roi-filter` on the patched crate's namespace.
