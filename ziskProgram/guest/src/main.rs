// OpenBlob ZisK proof — stub circuit.
//
// Reads the same fields OpenBlob.proofBlobDA hashes on-chain, recomputes
//   publicInputsHash = keccak256(abi.encode(blobhashes, hashedData, prevRoot,
//                                           newRoot, totalEtherPaid, blockHash))
// and commits it as the proof's public output. The validity bit is hardcoded
// to true — this is a placeholder for the real state-transition checks.

#![no_main]
ziskos::entrypoint!(main);

use alloy_primitives::{B256, U256};
use alloy_sol_types::SolValue;
use common::{GuestInputs, Output};
use tiny_keccak::{Hasher, Keccak};

fn main() {
    let inputs: GuestInputs = ziskos::io::read();

    let blobhashes: Vec<B256> = inputs.blobhashes.iter().copied().map(B256::from).collect();
    let hashed_data: Vec<B256> = inputs.hashed_data.iter().copied().map(B256::from).collect();
    let prev_root = B256::from(inputs.prev_root);
    let new_root = B256::from(inputs.new_root);
    let total_ether_paid = U256::from_be_bytes(inputs.total_ether_paid);
    let block_hash = B256::from(inputs.block_hash);

    // Mirror OpenBlob.proofBlobDA's
    //   keccak256(abi.encode(blobhashes, hashedData, prevRoot, newRoot,
    //                        totalEtherPaid, blockhash(blockNumber)))
    // `abi_encode_params` matches Solidity's multi-arg `abi.encode(a, b, ...)`
    // (head/tail layout with no outer offset).
    let encoded = (
        blobhashes,
        hashed_data,
        prev_root,
        new_root,
        total_ether_paid,
        block_hash,
    )
        .abi_encode_params();

    // Keccak-f routes through ZisK's `syscall_keccak_f` precompile because
    // tiny-keccak is patched at the workspace root.
    let mut hasher = Keccak::v256();
    hasher.update(&encoded);
    let mut public_inputs_hash = [0u8; 32];
    hasher.finalize(&mut public_inputs_hash);

    let output = Output {
        publicInputsHash: public_inputs_hash.into(),
        valid: true,
    };

    println!("publicInputsHash: {:02x?}", public_inputs_hash);

    ziskos::io::commit_slice(&output.abi_encode());
}
