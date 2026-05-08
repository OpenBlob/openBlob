use alloy_sol_types::sol;
use serde::{Deserialize, Serialize};

sol! {
    /// Public values committed by the OpenBlob ZisK proof.
    ///
    /// `publicInputsHash` is the keccak256 of the same digest the OpenBlob
    /// contract recomputes on-chain (see `OpenBlob.proofBlobDA`). The verifier
    /// compares this against the on-chain digest to bind the proof to a
    /// specific batch.
    ///
    /// `valid` is the circuit's verdict — for now this is a stub that always
    /// returns true; full state-transition checks come later.
    struct Output {
        bytes32 publicInputsHash;
        bool valid;
    }
}

/// Inputs to the OpenBlob proof guest, mirroring the fields the OpenBlob
/// contract feeds into its on-chain `keccak256(abi.encode(...))`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GuestInputs {
    pub blobhashes: Vec<[u8; 32]>,
    pub hashed_data: Vec<[u8; 32]>,
    pub prev_root: [u8; 32],
    pub new_root: [u8; 32],
    pub total_ether_paid: [u8; 32],
    pub block_hash: [u8; 32],
}

/// Hard-coded example batch used by the host driver and `build.rs`. Lets every
/// host binary (and the proof input file) stay in sync without duplicating the
/// values in five places.
pub fn sample_inputs() -> GuestInputs {
    GuestInputs {
        blobhashes: vec![[0xAAu8; 32], [0xBBu8; 32]],
        hashed_data: vec![[0x11u8; 32], [0x22u8; 32]],
        prev_root: [0u8; 32],
        new_root: [0xCCu8; 32],
        total_ether_paid: [0u8; 32],
        block_hash: [0xDDu8; 32],
    }
}
