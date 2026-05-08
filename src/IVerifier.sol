// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

/// @title IVerifier
/// @notice Interface for a ZK proof verifier.
/// @dev The caller commits all public inputs into a single digest off-chain (or
///      mirrors the on-chain digest computation) and passes it as
///      `publicInputsHash`. The circuit must constrain the same digest from its
///      public inputs.
interface IVerifier {
    /// @param proof             Encoded ZK proof bytes.
    /// @param publicInputsHash  keccak256 of the abi-encoded public inputs.
    /// @return True iff the proof is valid for the given digest.
    function verifyProof(bytes calldata proof, bytes32 publicInputsHash) external view returns (bool);
}
