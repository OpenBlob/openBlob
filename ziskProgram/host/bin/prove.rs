use alloy_primitives::{B256, U256};
use alloy_sol_types::SolValue;
use anyhow::Result;
use common::{GuestInputs, Output, sample_inputs};
use tiny_keccak::{Hasher, Keccak};
use zisk_sdk::{
    GuestProgram, Proof, ProverClient, PublicValues, ZiskStdin, load_program,
};

static PROGRAM: GuestProgram = load_program!("guest");

/// Compute the expected `publicInputsHash` natively (mirrors the guest and the
/// on-chain `OpenBlob.proofBlobDA` digest) so we can cross-check the proof's
/// committed public values against an off-chain reference.
fn expected_public_inputs_hash(inputs: &GuestInputs) -> [u8; 32] {
    let blobhashes: Vec<B256> = inputs.blobhashes.iter().copied().map(B256::from).collect();
    let hashed_data: Vec<B256> = inputs.hashed_data.iter().copied().map(B256::from).collect();
    let prev_root = B256::from(inputs.prev_root);
    let new_root = B256::from(inputs.new_root);
    let total_ether_paid = U256::from_be_bytes(inputs.total_ether_paid);
    let block_hash = B256::from(inputs.block_hash);

    let encoded = (
        blobhashes,
        hashed_data,
        prev_root,
        new_root,
        total_ether_paid,
        block_hash,
    )
        .abi_encode_params();

    let mut hasher = Keccak::v256();
    hasher.update(&encoded);
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting ZisK Prover Client...");

    let inputs = sample_inputs();
    let stdin = ZiskStdin::new();
    stdin.write(&inputs);
    println!(
        "Input prepared: {} blobs, {} hashedData",
        inputs.blobhashes.len(),
        inputs.hashed_data.len()
    );

    println!("Building prover client...");
    let client = ProverClient::embedded().build()?;

    println!("Setting up program...");
    client.upload(&PROGRAM).run()?;
    client.setup(&PROGRAM).run()?.await?;
    println!("Setup completed successfully");

    println!("Generating proof (this may take a while)...");
    let result = client.prove(&PROGRAM, stdin).run()?.await?;
    println!(
        "Proof generated successfully in {:?}",
        result.get_proving_time()
    );
    println!("Execution steps: {}", result.get_execution_steps());

    println!("Verifying proof...");
    result.verify()?;
    println!("Proof verification successful!");

    println!("Saving proof to disk...");
    result.save_proof("tmp/openblob_proof.bin")?;
    println!("Proof saved to tmp/openblob_proof.bin");

    let expected_hash = expected_public_inputs_hash(&inputs);
    let output = Output {
        publicInputsHash: expected_hash.into(),
        valid: true,
    };
    println!("Expected publicInputsHash: {:02x?}", expected_hash);

    println!("Verifying saved proof from disk...");
    let publics = PublicValues::write_abi(&output)?;
    let vk = PROGRAM.vk()?;
    let proof = Proof::load("tmp/openblob_proof.bin")?;
    proof.with_program_vk(&vk).with_publics(&publics).verify()?;
    println!("Proof verification successful!");

    println!("\u{2713} Successfully generated and verified all proofs!");

    Ok(())
}
