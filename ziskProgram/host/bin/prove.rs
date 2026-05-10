use anyhow::Result;
use common::{Output, compute_public_inputs_hash, sample_inputs, sum_ether};
use zisk_sdk::{
    GuestProgram, Proof, ProverClient, PublicValues, ZiskStdin, load_program,
};

static PROGRAM: GuestProgram = load_program!("guest");

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting ZisK Prover Client...");

    let inputs = sample_inputs();
    let stdin = ZiskStdin::new();
    stdin.write(&inputs);
    println!(
        "Input prepared: {} blobs, msg.sender={:02x?}",
        inputs.blobs.len(),
        inputs.msg_sender
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

    let expected_hash = compute_public_inputs_hash(&inputs);
    let expected_total_ether = sum_ether(&inputs.entry_auths);
    let output = Output {
        publicInputsHash: expected_hash.into(),
        valid: true,
        totalEtherAccumulated: expected_total_ether.into(),
    };
    println!("Expected publicInputsHash:      {:02x?}", expected_hash);
    println!("Expected totalEtherAccumulated: {:02x?}", expected_total_ether);

    println!("Verifying saved proof from disk...");
    let publics = PublicValues::write_abi(&output)?;
    let vk = PROGRAM.vk()?;
    let proof = Proof::load("tmp/openblob_proof.bin")?;
    proof.with_program_vk(&vk).with_publics(&publics).verify()?;
    println!("Proof verification successful!");

    println!("\u{2713} Successfully generated and verified all proofs!");

    Ok(())
}
