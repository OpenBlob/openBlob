---
name: zisk-new-program
description: Scaffold and write a new ZisK guest program — the Rust code that runs inside the ZisK zkVM and produces a zero-knowledge proof of its execution. Use whenever the user wants to start a new ZisK project, run `cargo-zisk new`, modify an existing Rust program to compile for ZisK, set up the guest/host project layout, configure the `ziskos` entrypoint, read inputs with `ziskos::io::read()`, commit public outputs with `ziskos::io::commit()`, build to the `riscv64ima-zisk-zkvm-elf` target, or test their guest in the ZisK emulator before generating a proof. Trigger even if the user only mentions "I want to prove X with ZisK", "how do I write a zkVM program", or "make my Rust program zk-provable".
---

# ZisK New Program

This skill creates a new ZisK guest program (the code being proven) and a host program (which feeds inputs and runs the prover), and walks the user through the small set of changes any Rust program needs to compile for the ZisK zkVM.

A working install is a prerequisite — if `cargo-zisk --version` fails, hand off to the **zisk-setup** skill before continuing.

---

## Step 1: Scaffold the project

`cargo-zisk new` creates a complete project tree with an example SHA-256 hasher. **Always start here** rather than building from scratch — the layout includes host/guest split, build scripts, and ready-made binaries for execute / prove / verify-constraints / compressed / plonk that are tedious to recreate.

```bash
cargo-zisk new <project-name>
cd <project-name>
```

The resulting tree:

```
<project-name>/
├── build.rs
├── Cargo.toml
├── .gitignore
├── guest/                     ← code that runs inside the zkVM
│   ├── src/main.rs
│   └── Cargo.toml
└── host/                      ← code that runs natively, drives the prover
    ├── src/main.rs
    ├── bin/
    │   ├── ziskemu.rs                ← run guest in the emulator
    │   ├── execute.rs                ← native-mode execute
    │   ├── verify-constraints.rs     ← check constraints without proving
    │   ├── prove.rs                  ← generate STARK proof
    │   ├── compressed.rs             ← recursive compression of an existing proof
    │   └── plonk.rs                  ← SNARK-wrap the compressed proof
    ├── build.rs
    └── Cargo.toml
```

Out of the box the example takes a `u32` input `n` and computes SHA-256 hash `n` times. Run it through to confirm everything works (see Step 4) before changing anything.

## Step 2: Write the guest program

The guest is a normal Rust program with **three changes**:

### a. Mark `main` as the ZisK entrypoint

At the top of `guest/src/main.rs`:

```rust
#![no_main]
ziskos::entrypoint!(main);
```

`#![no_main]` removes the default Rust entrypoint; the macro registers your `main` as the ZisK entry. Without these two lines the guest will not link.

### b. Add `ziskos` to the guest `Cargo.toml`

```toml
[dependencies]
ziskos = { git = "https://github.com/0xPolygonHermez/zisk.git" }
```

`ziskos` is the runtime crate that provides I/O, the entrypoint macro, profiling tags, and FFI hooks for hints and precompiles.

### c. Use `ziskos::io` for I/O instead of stdin/stdout/files

The zkVM has no filesystem, no stdin, and no `println!` for proofs. Inputs come in through `ziskos::io::read()`, public outputs go out through `ziskos::io::commit()`.

```rust
#![no_main]
ziskos::entrypoint!(main);

use sha2::{Digest, Sha256};

fn main() {
    // Read a u32 from input
    let n: u32 = ziskos::io::read();

    let mut hash = [0u8; 32];
    for _ in 0..n {
        let mut hasher = Sha256::new();
        hasher.update(hash);
        hash = hasher.finalize().into();
    }

    // Commit the hash as public output (verifier sees this)
    ziskos::io::commit(&hash);
}
```

**Read rules:**
- `ziskos::io::read::<T>()` works on any type `T: serde::Deserialize`.
- The order of `read` calls must match the order the host wrote them.
- Reading too many items will trap.

**Commit rules:**
- `ziskos::io::commit::<T>(&value)` works on any type `T: serde::Serialize`.
- Anything you commit becomes a **public output** — visible to anyone holding the proof. Don't commit secrets.
- Inputs are **private** by default; only what you commit becomes public.

### Variant: byte-slice input

For raw byte input (e.g., already-serialized data), use:

```rust
use ziskos::read_input_slice;
let bytes: &[u8] = read_input_slice();
```

This avoids a serde round-trip when the input is already in the right shape.

## Step 3: Build the guest to a RISC-V ELF

ZisK programs compile to the `riscv64ima-zisk-zkvm` target. Use `cargo-zisk build` (not `cargo build`) so the right target and linker flags are applied.

```bash
cargo-zisk build           # debug build, slower
cargo-zisk build --release # what you want for proving
```

Output paths:
- Debug: `./target/riscv64ima-zisk-zkvm-elf/debug/<guest-crate-name>`
- Release: `./target/elf/riscv64ima-zisk-zkvm-elf/release/<guest-crate-name>`

The ELF has no extension. From the `cargo-zisk new <name>` scaffold, that crate name is `guest`.

If you also want to test the program natively (no zkVM, just regular Rust), `cargo build` works on the guest because `ziskos` provides native fallbacks for its I/O functions.

## Step 4: Run in the ZisK emulator before proving

Proofs are slow. **Always run in the emulator first** to confirm the program is correct.

The scaffold's `host/bin/ziskemu.rs` already wires this up:

```bash
cargo run --release --bin ziskemu
```

Or call `ziskemu` directly with an explicit input file:

```bash
ziskemu \
  -e target/elf/riscv64ima-zisk-zkvm-elf/release/guest \
  -i host/tmp/input.bin
```

If the emulator stops with `EmulationNoCompleted`, the program needs more steps than the default budget. Raise it:

```bash
ziskemu -e <elf> -i <input> -n 10000000000   # 10B steps
```

The emulator prints any committed public outputs as `public 0`, `public 1`, etc. — these are 32-bit words; an output that's larger (e.g., a 32-byte hash) will appear as 8 consecutive `public N` lines.

For the build/prove/verify pipeline once the emulator is happy, hand off to the **zisk-tooling** skill.

---

## Project conventions worth following

- **Keep guest and host crates separate.** The scaffold puts them in `guest/` and `host/`. The host is regular Rust; the guest cross-compiles to RISC-V. They don't share dependencies — putting big native crates in the guest's `Cargo.toml` blows up compile time and ELF size.
- **Inputs go in `host/tmp/input.bin`.** The host binary is responsible for serializing inputs into that file before the prover runs.
- **`Cargo.toml` workspace setup matters.** The root `Cargo.toml` from `cargo-zisk new` already uses `[workspace] resolver = "2"`. Don't downgrade resolver to "1" or guest builds will pull conflicting native dependencies.
- **Heap size.** The default `ziskos` allocator is a bump allocator that never frees. This is ~10% faster than a normal heap, but you should size your data structures accordingly — pre-allocate with `Vec::with_capacity` rather than relying on growth, and avoid repeated short-lived allocations in hot loops.

## Common patterns

### Reading a custom struct

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct Block {
    height: u64,
    txs: Vec<[u8; 32]>,
}

fn main() {
    let block: Block = ziskos::io::read();
    // ... compute over block ...
    ziskos::io::commit(&block.height);
}
```

The host writes the same struct with `serde` and a matching `Serialize`. Use a shared crate (or just copy the type) so guest and host stay in sync.

### Multiple commits

```rust
ziskos::io::commit(&result_hash);
ziskos::io::commit(&counter);
```

Order matters — the verifier sees `public 0..7` for the hash, then `public 8` for the counter.

### Profile tags (for tuning later)

```rust
use ziskos::{profile_start, profile_end};

profile_start!(my_section);
expensive_operation();
profile_end!(my_section);
```

Zero overhead unless run under `ziskemu --profile-tags`. See the **zisk-profiling** workflow for the full picture; the tags themselves are cheap to leave in.

## What to *not* do in a guest program

- **No threads.** Determinism is required. Stick to single-threaded code.
- **No `HashMap` iteration in hot paths.** Random hash seeding makes iteration order non-deterministic, which breaks both proofs and (especially) hint generation.
- **No `println!` / `eprintln!` for results.** The verifier doesn't see them. Use `ziskos::io::commit`. (`println!` does work for debugging in the emulator though.)
- **No filesystem / network.** `std::fs`, `std::net`, etc., are absent or stubbed.
- **No `std::time::SystemTime::now()`.** The zkVM has no clock.

## Build/Run quick reference

```bash
# Scaffold
cargo-zisk new my_proof && cd my_proof

# Build
cargo-zisk build --release

# Test in emulator
cargo run --release --bin ziskemu

# When ready to prove → switch to zisk-tooling skill
```
