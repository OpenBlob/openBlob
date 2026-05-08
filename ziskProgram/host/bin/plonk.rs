use anyhow::Result;
use common::sample_inputs;
use zisk_sdk::{GuestProgram, Proof, ProofKind, ProverClient, ZiskStdin, load_program};

static PROGRAM: GuestProgram = load_program!("guest");

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting ZisK Prover Client (SNARK mode)...");

    let inputs = sample_inputs();
    let stdin = ZiskStdin::new();
    stdin.write(&inputs);
    println!(
        "Input prepared: {} blobs, {} hashedData",
        inputs.blobhashes.len(),
        inputs.hashed_data.len()
    );

    // Create a `ProverClient` method.
    println!("Building prover client with SNARK support...");
    let client = ProverClient::embedded().plonk().build()?;

    println!("Setting up program and generating verification key...");
    client.setup(&PROGRAM).run()?.await?;
    println!("Setup completed successfully");

    println!("Generating PLONK proof (this may take a while)...");
    let snark_proof = client
        .prove(&PROGRAM, stdin)
        .wrap(ProofKind::Plonk)
        .run()?
        .await?;
    println!(
        "PLONK proof generated successfully in {:?}",
        snark_proof.get_proving_time()
    );
    println!("Execution steps: {}", snark_proof.get_execution_steps());

    // Alternatively, it can also be done in two steps
    // let vadcop_result = client.prove(&PROGRAM, stdin)?.run()?;
    // let vkey = PROGRAM.vk()?;
    // let snark_proof = client.wrap_proof(vadcop_result.get_proof(), ProofMode::Plonk)?;

    println!("Verifying PLONK proof...");
    snark_proof.verify()?;
    println!("PLONK proof verification successful!");

    println!("Saving PLONK proof to disk...");
    snark_proof.save_proof("/tmp/openblob_proof_snark.bin")?;
    println!("Proof saved to /tmp/openblob_proof_snark.bin");

    println!("Loading and verifying saved PLONK proof...");
    let proof = Proof::load("/tmp/openblob_proof_snark.bin")?;
    let vkey = PROGRAM.vk()?;
    proof.with_program_vk(&vkey).verify()?;
    println!("Saved PLONK proof verification successful!");

    println!("\u{2713} Successfully generated and verified PLONK proof!");

    Ok(())
}
