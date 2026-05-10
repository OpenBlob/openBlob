use anyhow::Result;
use common::sample_inputs;
use zisk_sdk::{GuestProgram, ProfilingMode, ZiskStdin, load_program};

static PROGRAM: GuestProgram = load_program!("guest");

fn main() -> Result<()> {
    let inputs = sample_inputs();
    let stdin = ZiskStdin::new();
    stdin.write(&inputs);
    println!(
        "Input prepared: {} blobs, msg.sender={:02x?}",
        inputs.blobs.len(),
        inputs.msg_sender
    );

    println!("Running ZisK Emulator...");
    zisk_sdk::run(&PROGRAM, stdin, Some(ProfilingMode::Complete))?;
    println!("ZisK Emulator completed successfully!");

    Ok(())
}
