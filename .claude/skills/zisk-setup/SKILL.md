---
name: zisk-setup
description: Install and bootstrap ZisK (the high-performance zkVM by Polygon Hermez) on a fresh machine. Use this whenever the user wants to install ZisK, set up a ZisK development environment, run `ziskup`, build ZisK from source, install the ZisK Rust toolchain, set up proving keys / verify keys / SNARK keys, configure shared-memory / locked-memory limits, troubleshoot the "stddef.h not found" build error, or update an existing ZisK install. Trigger even if the user only mentions "install zk vm", "set up the zisk toolchain", "I cloned zisk and want to run it", or asks why `cargo-zisk` isn't on their PATH.
---

# ZisK Setup

This skill walks the user through getting a working ZisK install — system dependencies, the ZisK CLI (`cargo-zisk`, `ziskemu`, `zisk-coordinator`, `zisk-worker`), the `zisk` Rust toolchain that targets `riscv64ima-zisk-zkvm`, and the proving / verify keys.

There are two install paths. **Always prefer `ziskup` (the prebuilt-binary installer).** Building from source takes 45–60 minutes and is only worth it when contributing to ZisK itself or running on a platform where the binaries don't fit.

---

## Step 1: Confirm the platform

ZisK supports **Linux x86_64** and **macOS** only. On macOS, proof generation is slower because it isn't fully optimized yet — flag this to the user before proceeding.

- Ubuntu: 22.04 or higher
- macOS: 14 or higher, with Xcode + Homebrew installed

If the user is on something else (Windows, Linux ARM, older Ubuntu), tell them up front rather than letting an install fail mid-way.

## Step 2: Install system dependencies

The dependency list is long and ZisK builds will fail with cryptic errors if any of them are missing. Don't try to trim the list.

**Ubuntu:**
```bash
sudo apt-get install -y xz-utils jq curl build-essential qemu-system libomp-dev libgmp-dev nlohmann-json3-dev protobuf-compiler uuid-dev libgrpc++-dev libsecp256k1-dev libsodium-dev libpqxx-dev nasm libopenmpi-dev openmpi-bin openmpi-common libclang-dev clang gcc-riscv64-unknown-elf
```

**macOS:**
```bash
brew reinstall jq curl libomp protobuf openssl nasm pkgconf open-mpi libffi nlohmann-json libsodium riscv-tools
```

Rust and Git must be installed too — check with `rustc --version && git --version` before going further.

### Locked-memory limit (Linux only)

ZisK exchanges data between processes through shared memory and needs a high `MEMLOCK` limit. Verify with:

```bash
ulimit -l
```

If the result isn't `unlimited`, edit `/etc/systemd/system.conf` and add:

```
DefaultLimitMEMLOCK=infinity
```

Then **reboot** for the change to take effect. Without this, the prover will fail at runtime, often with confusing memory-allocation errors. It's worth doing up front even if `ulimit -l` shows a generous number.

## Step 3: Install ZisK with `ziskup` (recommended path)

```bash
curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash
```

The installer prompts the user to pick a setup option:

| # | Option | Use it when |
|---|---|---|
| 1 | **Install proving key** (default) | They want to generate or verify proofs locally — this is the right choice for almost everyone. |
| 2 | **Install proving key (no constant tree files)** | They have storage constraints and will generate constant tree files on first use. |
| 3 | **Install verify key** | They will only verify proofs (e.g., on a verifier-only host). |
| 4 | **None** | They only need to compile programs and run them in the emulator without any proving. |

The proving key is large; choosing option 4 then needing to prove later means a second download. Recommend (1) unless they have a specific reason.

**Non-interactive flags** for scripts/CI:

```bash
ziskup --provingkey   # equivalent to option 1
ziskup --verifykey    # option 3
ziskup --nokey        # option 4
```

For PLONK proving (used for SNARK wrapping):

```bash
ziskup setup_snark
```

### Verify the install

Two checks:

```bash
rustup toolchain list   # should include `zisk`
cargo-zisk --version    # should print a version, ending in [cpu] or [gpu]
```

If `cargo-zisk` isn't found, `~/.zisk/bin` isn't on `PATH`. The installer adds the export to `~/.bashrc` (or `~/.zshenv` on macOS) — they may need to open a new shell or `source` that file.

## Step 4: GPU support (optional, NVIDIA only)

`cargo-zisk --version` ends in `[gpu]` if GPU support is compiled in, `[cpu]` otherwise. To enable GPU proving:

1. Install [CUDA Toolkit](https://developer.nvidia.com/cuda-downloads).
2. **Build ZisK from source on the same host** that will run it — the build optimizes for the local GPU architecture, so prebuilt binaries don't get the speedup. (See "Building from Source" below.)
3. Add `--gpu` to `cargo-zisk prove` commands.

If the user installed via `ziskup` and wants GPU support, walk them through the source build path. Don't claim the prebuilt binaries support GPU.

## Step 5: Updating ZisK

```bash
ziskup
```

This re-runs the installer against the latest version. The same `--provingkey` / `--verifykey` / `--nokey` flags work for non-interactive updates.

## Step 6: Uninstall

```bash
rustup uninstall zisk
rm -rf $HOME/.zisk
```

---

## Building from Source

Only do this when:
- The user is contributing to ZisK
- They need GPU support (build on the GPU host)
- A prebuilt binary isn't available for their platform

**Allocate 45–60 minutes** for the setup-data step alone. Warn the user before starting.

### 1. Clone and build the tools

```bash
git clone https://github.com/0xPolygonHermez/zisk.git
cd zisk
cargo build --release
```

**Common Ubuntu error:**
```
/usr/lib/x86_64-linux-gnu/openmpi/include/mpi.h:237:10: fatal error: 'stddef.h' file not found
```
Fix:
```bash
find /usr -name "stddef.h"
# Then point the compiler at the directory it finds (e.g., gcc 13):
export C_INCLUDE_PATH=/usr/lib/gcc/x86_64-linux-gnu/13/include
export CPLUS_INCLUDE_PATH=$C_INCLUDE_PATH
cargo build --release
```

### 2. Install the binaries

```bash
mkdir -p $HOME/.zisk/bin
cp target/release/cargo-zisk target/release/ziskemu target/release/riscv2zisk \
   target/release/zisk-coordinator target/release/zisk-worker \
   target/release/libziskclib.a $HOME/.zisk/bin
```

### 3. (Linux x86_64 only) Copy assembly-rom files

Skip on macOS — assembly execution isn't supported there.

```bash
mkdir -p $HOME/.zisk/zisk/emulator-asm
cp -r ./emulator-asm/src $HOME/.zisk/zisk/emulator-asm
cp ./emulator-asm/Makefile $HOME/.zisk/zisk/emulator-asm
cp -r ./lib-c $HOME/.zisk/zisk
```

### 4. Add `~/.zisk/bin` to PATH

```bash
PROFILE=$([[ "$(uname)" == "Darwin" ]] && echo ".zshenv" || echo ".bashrc")
echo >>$HOME/$PROFILE && echo "export PATH=\"\$PATH:$HOME/.zisk/bin\"" >> $HOME/$PROFILE
source $HOME/$PROFILE
```

### 5. Install the ZisK Rust toolchain

```bash
cargo-zisk toolchain install
```

If the user wants to build the toolchain from source instead (e.g., for a custom RISC-V target):

```bash
cargo-zisk toolchain build
```

This requires the [Rust source build dependencies](https://github.com/rust-lang/rust/blob/master/INSTALL.md#dependencies).

### 6. Build setup files (only needed for proving)

NodeJS 20.x or higher is required. **This step takes 30–45 minutes.** If the user only wants to run programs in the emulator, skip it.

```bash
# In the parent folder of zisk/:
git clone https://github.com/0xPolygonHermez/pil2-compiler.git
git clone https://github.com/0xPolygonHermez/pil2-proofman.git
git clone https://github.com/0xPolygonHermez/pil2-proofman-js
(cd pil2-compiler && npm i)
(cd pil2-proofman-js && npm i)

cd zisk

# Generate fixed data
cargo run --release --bin arith_frops_fixed_gen
cargo run --release --bin binary_basic_frops_fixed_gen
cargo run --release --bin binary_extension_frops_fixed_gen

# Compile ZisK PIL
node --max-old-space-size=16384 ../pil2-compiler/src/pil.js pil/zisk.pil \
  -I pil,../pil2-proofman/pil2-components/lib/std/pil,state-machines,precompiles \
  -o pil/zisk.pilout -u tmp/fixed -O fixed-to-file

# Generate the proving key (the long step)
node --max-old-space-size=16384 --stack-size=8192 ../pil2-proofman-js/src/main_setup.js \
  -a ./pil/zisk.pilout -b build \
  -t ../pil2-proofman/pil2-components/lib/std/pil \
  -u tmp/fixed -r \
  -s ./state-machines/starkstructs.json

# Optional: SNARK wrapper
node ../pil2-proofman-js/src/main_setup_snark.js -b build \
  -t ../pil2-proofman/pil2-components/lib/std/pil \
  -f -w ../powersOfTau28_hez_final_27.ptau \
  -p ./state-machines/publics.json -n plonk

# Move the proving key into place
cp -R build/provingKey $HOME/.zisk
```

The `powersOfTau28_hez_final_27.ptau` file for the SNARK wrapper is a ~9 GB Powers of Tau ceremony file; download it separately if the user actually needs SNARK output.

---

## Troubleshooting checklist

When something is broken, run through these in order:

1. `cargo-zisk --version` works → install succeeded; the toolchain is on PATH.
2. `rustup toolchain list | grep zisk` → ZisK Rust toolchain is registered.
3. `ulimit -l` returns `unlimited` (Linux) → shared-memory limit is set.
4. `ls $HOME/.zisk/provingKey` exists → proving key is in place.
5. `cargo-zisk --version` ends in `[gpu]` → GPU support is built in (only matters if they need GPU).

If a fresh install behaves oddly, the user almost always needs to open a new shell so `~/.bashrc` / `~/.zshenv` is re-sourced.
