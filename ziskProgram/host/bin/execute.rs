use anyhow::Result;
use common::{Output, sample_inputs};
use zisk_sdk::{GuestProgram, ProverClient, ZiskStdin, load_program};

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
    client.setup(&PROGRAM).run()?.await?;
    println!("Setup completed successfully");

    println!("Executing program (no proof generation)...");
    let result = client.execute(&PROGRAM, stdin.clone()).run()?.await?;

    println!("\u{2713} Execution completed successfully!");
    println!("Cycles: {}", result.get_execution_steps());
    println!("Duration: {:?}", result.get_execution_time());

    println!("Reading public outputs...");
    let output: Output = result.get_public_values_abi()?;
    println!("Public outputs:");
    println!("  publicInputsHash:      {:02x?}", output.publicInputsHash);
    println!("  valid:                 {}", output.valid);
    println!("  totalEtherAccumulated: {:02x?}", output.totalEtherAccumulated);

    Ok(())
}
