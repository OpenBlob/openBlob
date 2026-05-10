// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

import {IVerifier} from "./IVerifier.sol";

/// @title OpenBlob
/// @notice ZK-proven data-availability rollup using EIP-4844 blobs as the DA layer.
/// @dev Users deposit ETH on L1 as collateral; off-chain provers reference past
///      blobs by their (L1 blockNumber, txIndex) coordinate and submit a ZK
///      proof binding those blobs, the old state and the proposed new state via
///      {proofBlobDA}. State advances iff the proof verifies.
///
///      Blob payload schema (off-chain, not represented on-chain):
///      each blob serializes a tuple `(address[] users, uint256[] newNonces,
///      uint256[] newBalances)`. The ZK circuit is responsible for retrieving
///      the blob at each supplied `(blockNumber, txIndex)` and binding its
///      contents into the state transition; this contract treats the
///      coordinates opaquely and only forwards them into the public-inputs
///      digest.
contract OpenBlob {
    /// @notice (L1 block number, transaction index) coordinate of a blob being
    ///         proven against. Packed into a single 32-byte slot.
    struct BlobRef {
        uint64 blockNumber;
        uint64 txIndex;
    }

    /// @notice ZK verifier used to validate state-transition proofs.
    IVerifier public immutable verifier;

    /// @notice ETH deposited per address (L1 collateral pool, cumulative).
    /// @dev Never decremented on-chain; payouts go to the prover via
    ///      `totalEtherPaid`. The canonical "spent" amount per account lives
    ///      inside {openBlobRoot}.
    mapping(address => uint256) public balances;

    /// @notice Merkle root committing to (nonce, amountExpended) for every account.
    bytes32 public openBlobRoot;

    /// @notice Caller-supplied `prevRoot` does not match the current {openBlobRoot}.
    /// @param expected The current on-chain root.
    /// @param provided The root the caller passed in.
    error PrevRootMismatch(bytes32 expected, bytes32 provided);
    /// @notice The verifier rejected the proof.
    error InvalidProof();
    /// @notice ETH transfer to `msg.sender` failed.
    error TransferFailed();

    /// @notice Emitted when ETH is deposited.
    /// @param from   Beneficiary credited.
    /// @param amount Amount of ETH credited.
    event Deposit(address indexed from, uint256 amount);

    /// @notice Emitted on a successful state transition.
    /// @param prover         Caller of {proofBlobDA} (also receives `totalExtracted`).
    /// @param newRoot        New {openBlobRoot}.
    /// @param totalExtracted Amount of ETH paid out to the prover this batch.
    /// @param blockNumber    L1 block number whose blockhash was bound into the proof.
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

    /// @notice Deposit ETH and credit `beneficiary`'s collateral balance.
    function deposit(address beneficiary) external payable {
        balances[beneficiary] += msg.value;
        emit Deposit(beneficiary, msg.value);
    }

    /// @notice Verify a batch of blobs, advance the state root, and pay the prover.
    /// @dev All public inputs are collapsed into a single digest:
    ///      ```
    ///      publicInputsHash = keccak256(abi.encode(
    ///          blobsReference,              // BlobRef[] (blockNumber, txIndex per blob)
    ///          prevRoot,                    // == openBlobRoot at call time
    ///          newRoot,
    ///          totalEtherPaid,
    ///          blockhash(blockNumber),      // L1 blockhash, must be within last 256
    ///          msg.sender                   // binds proof to submitter
    ///      ))
    ///      ```
    ///      The ZK circuit must constrain the same digest from its own public
    ///      inputs and is responsible for fetching each referenced blob's
    ///      contents, enforcing replay/uniqueness within the batch, and
    ///      cross-batch invariants (monotonic block numbers, per-user
    ///      `expended <= deposited`, etc.). This function only rotates
    ///      `openBlobRoot` and forwards `totalEtherPaid` ETH to the caller.
    /// @param blobsReference Array of (blockNumber, txIndex) coordinates of the
    ///                       blobs being proven against.
    /// @param prevRoot       Caller's view of the current {openBlobRoot}; must
    ///                       equal it exactly or the call reverts.
    /// @param newRoot        Proposed new {openBlobRoot}.
    /// @param totalEtherPaid ETH amount paid to `msg.sender` on success.
    /// @param blockNumber    L1 block number whose `blockhash` is bound into
    ///                       the digest (must be within the last 256 blocks).
    /// @param proof          Encoded ZK proof bytes.
    function proofBlobDA(
        BlobRef[] calldata blobsReference,
        bytes32 prevRoot,
        bytes32 newRoot,
        uint256 totalEtherPaid,
        uint256 blockNumber,
        bytes calldata proof
    ) external {
        // The caller asserts which root the proof was generated against. Reject
        // if it has drifted from on-chain state — otherwise a stale proof could
        // land after another submission rotated the root.
        bytes32 currentRoot = openBlobRoot;
        if (prevRoot != currentRoot) revert PrevRootMismatch(currentRoot, prevRoot);

        // Collapse all public inputs into one digest the circuit must reproduce.
        // `msg.sender` is bound in to prevent another address from lifting the
        // proof out of the mempool and re-submitting it to claim `totalEtherPaid`.
        bytes32 publicInputsHash = keccak256(
            abi.encode(
                blobsReference,
                prevRoot,
                newRoot,
                totalEtherPaid,
                blockhash(blockNumber),
                msg.sender
            )
        );

        if (!verifier.verifyProof(proof, publicInputsHash)) revert InvalidProof();

        // Effects before interactions. Not reentrancy-sensitive: `openBlobRoot`
        // is bound into the digest above and rotated here, so any reentrant
        // call would need a fresh proof against the new root.
        openBlobRoot = newRoot;

        (bool sent, ) = payable(msg.sender).call{value: totalEtherPaid}("");
        if (!sent) revert TransferFailed();

        emit BlobDAProved(msg.sender, newRoot, totalEtherPaid, blockNumber);
    }
}
