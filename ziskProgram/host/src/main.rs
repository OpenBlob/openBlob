use anyhow::Result;
use common::sample_inputs;
use zisk_sdk::{GuestProgram, ProverClient, ZiskStdin, load_program};

static PROGRAM: GuestProgram = load_program!("guest");

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting ZisK Prover Client...");

    let client = ProverClient::embedded().build()?;

    client.upload(&PROGRAM).run()?;
    client.setup(&PROGRAM).run()?.await?;

    let inputs = sample_inputs();
    let stdin = ZiskStdin::new();
    stdin.write(&inputs);
    println!(
        "Input prepared: {} blobs, prevRoot={:02x?}",
        inputs.blobs.len(),
        inputs.prev_root
    );

    let handle = client.execute(&PROGRAM, stdin.clone()).run()?;
    let result = handle.await?;

    println!(
        "ZisK has executed program with {} cycles in {:?} ms",
        result.get_execution_steps(),
        result.get_execution_time()
    );

    let prove_handle = client.prove(&PROGRAM, stdin.clone()).run()?;
    let vadcop_result = prove_handle.await?;

    let vkey = PROGRAM.vk()?;
    vadcop_result.with_program_vk(&vkey).verify()?;

    println!("successfully generated and verified proof for the program!");

    Ok(())
}
