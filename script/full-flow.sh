#!/usr/bin/env bash
# End-to-end orchestrator: deploy mock + OpenBlob, A deposits 0.1 ETH, A signs
# a transfer to B, B posts a type-3 blob tx that proves it and claims the ETH.
#
# Required env:
#   RPC_URL        e.g. http://localhost:8545
#   ACTOR_A_PK     depositor / signer
#   ACTOR_B_PK     prover / claimant
#
# Optional env:
#   DEPOSIT_AMOUNT_WEI   default 0.1 ether
#   OPEN_BLOB_ADDR       reuse an existing deployment instead of redeploying

set -euo pipefail

: "${RPC_URL:?set RPC_URL}"
: "${ACTOR_A_PK:?set ACTOR_A_PK}"
: "${ACTOR_B_PK:?set ACTOR_B_PK}"

DEPOSIT_AMOUNT_WEI="${DEPOSIT_AMOUNT_WEI:-100000000000000000}" # 0.1 ether
ART_DIR="./flow-artifacts"
mkdir -p "$ART_DIR"

ACTOR_A=$(cast wallet address --private-key "$ACTOR_A_PK")
ACTOR_B=$(cast wallet address --private-key "$ACTOR_B_PK")
CHAIN_ID=$(cast chain-id --rpc-url "$RPC_URL")

echo "Actor A: $ACTOR_A"
echo "Actor B: $ACTOR_B"
echo "Chain id: $CHAIN_ID"

# 1. Deploy (mock verifier + OpenBlob) unless an address was supplied.
if [[ -z "${OPEN_BLOB_ADDR:-}" ]]; then
    echo "==> deploy mock verifier + OpenBlob"
    forge script script/Deploy.s.sol:DeployOpenBlobMock \
        --rpc-url "$RPC_URL" --private-key "$ACTOR_A_PK" --broadcast -vv >/dev/null
    OPEN_BLOB_ADDR=$(jq -r \
        '.transactions[] | select(.contractName == "OpenBlob") | .contractAddress' \
        "broadcast/Deploy.s.sol/$CHAIN_ID/run-latest.json")
fi
export OPEN_BLOB_ADDR
echo "OpenBlob: $OPEN_BLOB_ADDR"

# 2. Actor A deposits.
echo "==> A deposits $DEPOSIT_AMOUNT_WEI wei"
DEPOSIT_AMOUNT_WEI="$DEPOSIT_AMOUNT_WEI" \
    forge script script/Deposit.s.sol:Deposit \
    --rpc-url "$RPC_URL" --broadcast -vv >/dev/null
A_BALANCE=$(cast call "$OPEN_BLOB_ADDR" "balances(address)(uint256)" "$ACTOR_A" --rpc-url "$RPC_URL")
echo "On-chain balance for A: $A_BALANCE"

# 3. Actor A signs a transfer authorization to B (off-chain, no broadcast).
echo "==> A signs transfer of $DEPOSIT_AMOUNT_WEI to B"
RECIPIENT="$ACTOR_B" \
AMOUNT_WEI="$DEPOSIT_AMOUNT_WEI" \
OUT_FILE="$ART_DIR/transfer.json" \
    forge script script/SignTransfer.s.sol:SignTransfer -vv >/dev/null

# 4. Actor B builds the blob payload + proofBlobDA calldata.
echo "==> B prepares blob + calldata"
PREV_ROOT=$(cast call "$OPEN_BLOB_ADDR" "openBlobRoot()(bytes32)" --rpc-url "$RPC_URL")
PROVE_BLOCK_NUMBER=$(cast block-number --rpc-url "$RPC_URL")
TRANSFER_FILE="$ART_DIR/transfer.json" \
BLOB_FILE="$ART_DIR/blob.bin" \
CALLDATA_FILE="$ART_DIR/proof.calldata" \
PREV_ROOT="$PREV_ROOT" \
PROVE_BLOCK_NUMBER="$PROVE_BLOCK_NUMBER" \
TOTAL_ETHER_PAID="$DEPOSIT_AMOUNT_WEI" \
    forge script script/PrepareProof.s.sol:PrepareProof -vv >/dev/null

CALLDATA=$(cat "$ART_DIR/proof.calldata")

# 5. B sends the type-3 blob tx. cast handles KZG commitment + versioned hash.
echo "==> B sends blob tx"
cast send --rpc-url "$RPC_URL" --private-key "$ACTOR_B_PK" \
    --blob --path "$ART_DIR/blob.bin" \
    "$OPEN_BLOB_ADDR" "$CALLDATA"

NEW_ROOT=$(cast call "$OPEN_BLOB_ADDR" "openBlobRoot()(bytes32)" --rpc-url "$RPC_URL")
B_ETH=$(cast balance --rpc-url "$RPC_URL" "$ACTOR_B")
echo
echo "Done."
echo "openBlobRoot: $NEW_ROOT"
echo "Actor B ETH:  $B_ETH wei"
