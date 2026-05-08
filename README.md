# BlobShare

Minimal Foundry project exposing the EVM `BLOBHASH` opcode (EIP-4844) through a tiny `BlobShare` contract.

```solidity
function blob(uint256 index) external view returns (bytes32);
```

Returns the versioned blob hash at `index` for the current transaction (or `0x00` when no blob is attached at that index).

## Layout

```
.
├── foundry.toml
├── remappings.txt
├── src/BlobShare.sol         # the contract
├── script/BlobShare.s.sol    # deployment script
└── test/BlobShare.t.sol      # forge tests
```

## Setup

```bash
forge install foundry-rs/forge-std --no-commit
forge build
forge test -vv
```

## Usage

Deploy:

```bash
forge script script/BlobShare.s.sol:BlobShareScript \
  --rpc-url <RPC> \
  --private-key <KEY> \
  --broadcast
```

Read blob 0 of a tx that carries blobs:

```bash
cast call <BLOB_SHARE_ADDR> "blob(uint256)(bytes32)" 0
```

Note: `BLOBHASH` only returns non-zero for blob-carrying transactions (type 0x03). Regular calls return `0x00`. Tests use `vm.blobhashes(...)` to inject hashes.
