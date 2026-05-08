---
name: zisk-tooling
description: Drive the ZisK CLI workflow — `cargo-zisk` (build, program-setup, verify-constraints, prove, verify) and `ziskemu` (emulate, max-steps, metrics, stats), plus running multi-process / GPU proofs and the scaffold's `cargo run --release --bin <prove|compressed|plonk|verify-constraints|ziskemu>` shortcuts. Use whenever the user wants to build a guest ELF, run program setup, generate or verify a proof, troubleshoot proving errors like missing proving keys or `EmulationNoCompleted`, scale proving across CPU cores via `mpirun`, switch to GPU proving, generate a compressed/PLONK proof, or just figure out which `cargo-zisk` subcommand to use. Trigger even if the user only mentions "run my zisk proof", "generate a STARK", "make my proof smaller", "use multiple cores for proving", or "verify this proof file".
---

# ZisK Tooling

Once a guest program compiles (see **zisk-new-program**), the `cargo-zisk` and `ziskemu` CLIs handle everything else: setup, emulation, constraint checking, proving, verification, and proof compression. This skill is the reference for which command to run, what flags matter, and where outputs land.

For the install and toolchain setup, see **zisk-setup**.

---

## The end-to-end flow

```
cargo-zisk build --release
  └─→ guest ELF at target/elf/riscv64ima-zisk-zkvm-elf/release/<crate>

cargo-zisk program-setup -e <elf>
  └─→ writes program-specific files into ~/.zisk/cache

ziskemu -e <elf> -i <input>                       (sanity check, fast)
cargo-zisk verify-constraints -e <elf> -i <input> (validates constraints, no proof)
cargo-zisk prove -e <elf> -i <input> -o proof     (the real proof; minutes)
cargo-zisk verify -p proof/vadcop_final_proof.bin (anyone can run this)
```

The scaffold from `cargo-zisk new` exposes each of these as a `cargo run --release --bin <name>` so you don't have to re-type the paths. Use the binaries when iterating in one project; use the raw `cargo-zisk` CLI when scripting across projects or in CI.

---

## Building

```bash
cargo-zisk build            # debug
cargo-zisk build --release  # what you want for proving
```

Output: `target/elf/riscv64ima-zisk-zkvm-elf/release/<crate>` (release) or `target/riscv64ima-zisk-zkvm-elf/debug/<crate>` (debug). No file extension.

The ELF re-builds whenever the guest source changes. Anything downstream (program-setup, prove) keys off the ELF — re-run `program-setup` after every rebuild.

## Program setup

Required **once per ELF**. If the ELF changes, re-run.

```bash
cargo-zisk program-setup \
  -e target/elf/riscv64ima-zisk-zkvm-elf/release/guest \
  -k $HOME/.zisk/provingKey
```

Flags:
- `-e` / `--elf` — the guest ELF.
- `-k` / `--proving-key` — directory with the proving key. Defaults to `~/.zisk/provingKey`; only set if you have keys somewhere else.

Setup files land in `~/.zisk/cache`. To clean them out:

```bash
cargo-zisk utils clean-cache --all
```

Skipping this step is the most common cause of "everything fails after I changed the program" — the cache still references the old ELF.

## Emulation (`ziskemu`)

`ziskemu` runs the guest without proving. **Always your first stop** — it's fast and surfaces correctness bugs without paying for a proof.

```bash
ziskemu \
  -e target/elf/riscv64ima-zisk-zkvm-elf/release/guest \
  -i host/tmp/input.bin
```

Useful flags:

| Flag | Purpose |
|---|---|
| `-e` / `--elf` | ELF path |
| `-i` / `--inputs` | Input file (binary) |
| `-n` / `--max-steps` | Override default step budget. Pass e.g. `10000000000` if you hit `EmulationNoCompleted`. |
| `-m` / `--log-metrics` | Throughput, clocks/step, total duration |
| `-X` / `--stats` | Full cost breakdown by category and opcode |
| `-S` / `--read-symbols` | Load function names for ROI / profiling output |
| `--sdk` | Compact "report" view (good for CI logs) |
| `--steps` | Just the step count, nothing else |
| `--profiler-output <file>` | Export a Firefox Profiler JSON for visualization |

If `ziskemu` fails with `EmulationNoCompleted`, the program ran out of step budget. Raise `-n`. If it loops forever, the program has a real infinite loop — fix the program, not the budget.

For deep profiling and optimization, see the dedicated profiling docs at `book/developer/profiling.md` (covered tooling: `--sdk`, `--top-functions`, `--roi-filter`, `-D`, `-H`, `--profiler-output`).

## Verifying constraints (no proof)

This is the dress rehearsal for `prove`. It executes the program, builds the trace, and checks every constraint — all the failure modes of a real proof, in a fraction of the time.

```bash
cargo-zisk verify-constraints \
  -e target/elf/riscv64ima-zisk-zkvm-elf/release/guest \
  -i host/tmp/input.bin \
  -k $HOME/.zisk/provingKey
```

A green run prints lines like:

```
[INFO ] CstrVrfy: ··· ✓ All global constraints were successfully verified
[INFO ] CstrVrfy: ··· ✓ All constraints were verified
```

If constraints fail here, **don't run `prove`** — it will fail in the same place but slower. Fix the guest first.

## Proving

The real thing:

```bash
cargo-zisk prove \
  -e target/elf/riscv64ima-zisk-zkvm-elf/release/guest \
  -i host/tmp/input.bin \
  -k $HOME/.zisk/provingKey \
  -o proof
```

Outputs end up in `./proof`. The final proof is `proof/vadcop_final_proof.bin`.

Expect minutes. Memory usage is around 25 GB per process for the Rust emulator path, more for assembly. Check free RAM before kicking it off.

## Verifying a proof

```bash
cargo-zisk verify \
  -p ./proof/vadcop_final_proof.bin \
  -k $HOME/.zisk/provingKey
```

`-p` is the only flag the user usually has to set; the rest default to keys in `~/.zisk`.

This is what a verifier (a third party, smart contract relay, etc.) would run. It's cheap.

## Compressing / SNARK-wrapping a proof

The scaffold has two extra binaries for size reduction:

```bash
cargo run --release --bin compressed   # recursive compression on top of the STARK
cargo run --release --bin plonk        # SNARK wrap (requires provingKeySnark)
```

Use `compressed` when you want a smaller proof but still STARK-verifiable. Use `plonk` when something downstream (an EVM verifier contract) expects a PLONK proof — note this requires `ziskup setup_snark` to have installed the SNARK proving key first.

---

## Multi-process proofs (`mpirun`)

Proofs scale well across processes. ZisK uses MPI so the same command works on one host or across many.

```bash
mpirun --bind-to none \
  -np <num_processes> \
  -x OMP_NUM_THREADS=<threads_per_process> \
  -x RAYON_NUM_THREADS=<threads_per_process> \
  target/release/cargo-zisk \
  prove -e <elf> -i <input> -k <proving-key> -o proof
```

Sizing rules:

- **Total work units** = `num_processes × threads_per_process`. Match to physical cores (× 2 with hyperthreading).
- **Splitting helps.** On a single multi-core host, `2 × 32` usually beats `1 × 64`. Concurrency at the process level helps even within one machine.
- **Memory scales linearly** with processes. ~25 GB per process; verify total RAM is ≥ `25 × num_processes` before launching.
- **`--bind-to none`** lets the OS schedule freely. Don't pin unless you're benchmarking.

For full distributed clusters (separate coordinator + worker hosts under systemd), see the distributed-execution guide at `book/getting_started/distributed_execution.md`.

## GPU proving

GPU proving requires:
1. NVIDIA GPU + CUDA Toolkit installed
2. ZisK **built from source on the GPU host** — prebuilt binaries are CPU-only
3. `cargo-zisk --version` ends in `[gpu]`

Then add `--gpu` to any `prove` invocation:

```bash
cargo-zisk prove -e <elf> -i <input> -k <proving-key> -o proof --gpu
```

Combine with `mpirun` for multi-GPU / multi-process, but watch GPU memory — it's tighter than system RAM, and OOM errors on a GPU manifest as cryptic CUDA failures.

---

## Quick command map

| Goal | Command |
|---|---|
| Build guest ELF | `cargo-zisk build --release` |
| First-time setup for an ELF | `cargo-zisk program-setup -e <elf>` |
| Sanity-check the guest | `ziskemu -e <elf> -i <input>` (or `cargo run --release --bin ziskemu`) |
| Cost / profiling stats | `ziskemu -e <elf> -i <input> -X -S` |
| Validate constraints (no proof) | `cargo-zisk verify-constraints -e <elf> -i <input>` |
| Generate STARK proof | `cargo-zisk prove -e <elf> -i <input> -o proof` |
| Verify a proof | `cargo-zisk verify -p proof/vadcop_final_proof.bin` |
| Compress a proof | `cargo run --release --bin compressed` |
| SNARK-wrap | `cargo run --release --bin plonk` (after `ziskup setup_snark`) |
| Multi-process prove | `mpirun --bind-to none -np N -x OMP_NUM_THREADS=T -x RAYON_NUM_THREADS=T target/release/cargo-zisk prove ...` |
| GPU prove | add `--gpu` |
| Clear program-setup cache | `cargo-zisk utils clean-cache --all` |

---

## Troubleshooting

**`EmulationNoCompleted` from `ziskemu`** — bump `-n` (max steps), e.g., `-n 10000000000`. If it still doesn't finish, the program is genuinely too long or has an infinite loop.

**`prove` fails immediately, says proving key missing** — `~/.zisk/provingKey` isn't there. Re-run `ziskup` (option 1) or pass `-k` to a custom location.

**`prove` fails after `verify-constraints` passed** — usually a stale program-setup cache. Run `cargo-zisk utils clean-cache --all` and re-run `program-setup`.

**Proofs randomly OOM** — drop `num_processes` (each takes ~25 GB) or, for GPU, drop `--max-streams`.

**`cargo-zisk: command not found`** — `~/.zisk/bin` isn't on `PATH`. Open a new shell or `source ~/.bashrc` (`~/.zshenv` on macOS).

**`cargo-zisk --version` ends in `[cpu]` but you wanted GPU** — install CUDA, then build from source on the GPU host. Prebuilt binaries are always CPU-only.
