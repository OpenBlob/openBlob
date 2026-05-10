// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

import {IVerifier} from "../IVerifier.sol";

/// @title OpenBlob
/// @notice ZK-proven data-availability rollup using EIP-4844 blobs as the DA layer.
/// @dev Users deposit ETH on L1 as collateral; off-chain provers post blobs whose
///      payloads carry batched account updates and submit a ZK proof binding the
///      blob hashes, the blob contents, the old state and the proposed new state
///      via {proofBlobDA}. State advances iff the proof verifies.
///
///      Blob payload schema (off-chain, not represented on-chain):
///      each blob serializes a tuple `(address[] users, uint256[] newNonces,
///      uint256[] newBalances)`. The prover hashes that payload (e.g.
///      `keccak256(abi.encode(users, newNonces, newBalances))`) and passes the
///      digest through `hashedData[i]`; the ZK circuit must re-derive the same
///      digest from the blob contents. This contract treats `hashedData[i]` as
///      opaque and only registers it in {dataAvailable} on a successful proof.
contract OpenBlob {
    /// @notice ZK verifier used to validate state-transition proofs.
    IVerifier public immutable verifier;

    /// @notice ETH deposited per address (L1 collateral pool).
    mapping(address => uint256) public balances;

    /// @notice Registry of blob-data digests that have been proven available.
    /// @dev Set to true the first time a `hashedData` entry passes proof verification.
    mapping(bytes32 => bool) public dataAvailable;

    /// @notice Merkle root committing to (nonce, amountExpended) for every account.
    bytes32 public openBlobRoot;

    /// @notice `blobIndexes` length does not match `hashedData` length.
    error LengthMismatch();
    /// @notice Caller-supplied `prevRoot` does not match the current {openBlobRoot}.
    /// @param expected The current on-chain root.
    /// @param provided The root the caller passed in.
    error PrevRootMismatch(bytes32 expected, bytes32 provided);
    /// @notice `blobIndexes` are not strictly increasing.
    /// @param previous The previous (lower-bounding) index.
    /// @param current  The offending index.
    error BlobIndexNotIncreasing(uint256 previous, uint256 current);
    /// @notice `blobhash(index)` returned zero — blob not present in the calling tx.
    /// @param index The missing blob index.
    error BlobNotFound(uint256 index);
    /// @notice The verifier rejected the proof.
    error InvalidProof();
    /// @notice ETH transfer to `msg.sender` failed.
    error TransferFailed();

    /// @notice Emitted when ETH is deposited.
    /// @param from   Depositor.
    /// @param amount Amount of ETH credited.
    event Deposit(address indexed from, uint256 amount);

    /// @notice Emitted on a successful state transition.
    /// @param prover         Caller of {proofBlobDA} (also receives `totalExtracted`).
    /// @param newRoot        New {openBlobRoot}.
    /// @param totalExtracted Amount of ETH paid out to the prover this batch.
    /// @param blockNumber    L2 block number proven this batch.
    event BlobDAProved(
        address indexed prover,
        bytes32 newRoot,
        uint256 totalExtracted,
        uint256 blockNumber
    );

    /// @param _verifier Address of the deployed ZK verifier contract.
    constructor(IVerifier _verifier) {
        verifier = _verifier;
    }

    /// @notice Deposit ETH and credit the caller's collateral balance.
    function deposit(address beneficiary) external payable {
        balances[beneficiary] += msg.value;
        emit Deposit(beneficiary, msg.value);
    }

    /// @notice Verify a batch of blobs, advance the state root, and pay the prover.
    /// @dev All public inputs are collapsed into a single digest:
    ///      ```
    ///      publicInputsHash = keccak256(abi.encode(
    ///          blobhashes,                 // bytes32[] of blobhash(blobIndexes[i])
    ///          hashedData,                 // bytes32[] supplied by the caller
    ///          prevRoot,                   // == openBlobRoot at call time
    ///          newRoot,
    ///          totalEtherPaid,
    ///          blockhash(blockNumber)      // L1 blockhash, must be within last 256
    ///      ))
    ///      ```
    ///      The ZK circuit must constrain the same digest from its own public
    ///      inputs. Each `hashedData[i]` MUST be the digest the circuit derives
    ///      from blob `blobIndexes[i]`'s contents; this contract treats it as
    ///      opaque and, on success, records it in {dataAvailable}. The circuit
    ///      is responsible for cross-batch invariants (monotonic block numbers,
    ///      per-user `expended <= deposited`, etc.); this function only stores
    ///      `openBlobRoot` and forwards `totalEtherPaid` ETH to the
    ///      caller.
    /// @param blobIndexes    Indices into the calling tx's blob set.
    /// @param hashedData     Digest of each blob's payload, parallel to `blobIndexes`.
    /// @param prevRoot       Caller's view of the current {openBlobRoot}; must
    ///                       equal it exactly or the call reverts. Bound into
    ///                       the public-inputs digest so the proof commits to
    ///                       the state it was generated against.
    /// @param newRoot        Proposed new {openBlobRoot}.
    /// @param totalEtherPaid ETH amount paid to `msg.sender` on success;
    ///                       not stored on-chain.
    /// @param blockNumber    L2 block number proven this batch; the L1
    ///                       `blockhash(blockNumber)` is bound into the digest.
    /// @param proof          Encoded ZK proof bytes.
    function proofBlobDA(
        uint256[] calldata blobIndexes,
        bytes32[] calldata hashedData,
        bytes32 prevRoot,
        bytes32 newRoot,
        uint256 totalEtherPaid,
        uint256 blockNumber,
        bytes calldata proof
    ) external {
        // hashedData must line up 1-to-1 with the blobs being proven.
        uint256 blobsLength = blobIndexes.length;
        if (blobsLength != hashedData.length) revert LengthMismatch();

        // The caller asserts which root the proof was generated against. Reject
        // if it has drifted from on-chain state — otherwise a stale proof could
        // land after another submission rotated the root.
        bytes32 currentRoot = openBlobRoot;
        if (prevRoot != currentRoot) revert PrevRootMismatch(currentRoot, prevRoot);

        // Resolve every blobhash and require strictly-increasing indexes so the
        // (blobhashes, hashedData) pair has a single canonical ordering — this
        // prevents replays where the same blob is claimed twice and also pins
        // down the input to the public-inputs hash.
        bytes32[] memory blobhashes = new bytes32[](blobsLength);
        uint256 lastIndex;
        for (uint256 i = 0; i < blobsLength; i++) {
            uint256 idx = blobIndexes[i];
            if (i > 0 && idx <= lastIndex) revert BlobIndexNotIncreasing(lastIndex, idx);
            bytes32 h = blobhash(idx);
            if (h == bytes32(0)) revert BlobNotFound(idx);
            blobhashes[i] = h;
            lastIndex = idx;
        }

        // Collapse all public inputs into one digest the circuit must reproduce.
        // `prevRoot` (already checked == openBlobRoot above) binds the proof to
        // the previous state.
        bytes32 publicInputsHash = keccak256(
            abi.encode(
                blobhashes,
                hashedData,
                prevRoot,
                newRoot,
                totalEtherPaid,
                blockhash(blockNumber),
                msg.sender
            )
        );

        if (!verifier.verifyProof(proof, publicInputsHash)) revert InvalidProof();

        // Effects before interactions. Note: this function is NOT reentrancy-
        // sensitive — `openBlobRoot` is bound into the digest above and rotated
        // here, so any reentrant call would need a fresh proof against the new
        // root, which the attacker cannot forge.
        for (uint256 i = 0; i < blobsLength; i++) {
            dataAvailable[hashedData[i]] = true;
        }
        openBlobRoot = newRoot;

        // Pay the prover. `totalEtherPaid` is consumed here and not
        // stored — the verifier is the sole authority on what the amount is.
        (bool sent, ) = payable(msg.sender).call{value: totalEtherPaid}("");
        if (!sent) revert TransferFailed();

        emit BlobDAProved(msg.sender, newRoot, totalEtherPaid, blockNumber);
    }
}
