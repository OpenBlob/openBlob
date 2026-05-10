use anyhow::{Context, Result};
use blob_sync::FetchConfig;
use common::{BlobRefIn, GuestInputs, compute_public_inputs_hash, sample_inputs};
use url::Url;
use zisk_sdk::{GuestProgram, ProverClient, ZiskStdin, load_program};

static PROGRAM: GuestProgram = load_program!("guest");

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting ZisK Prover Client...");

    let inputs = build_inputs().await?;

    let client = ProverClient::embedded().build()?;
    client.upload(&PROGRAM).run()?;
    client.setup(&PROGRAM).run()?.await?;

    let stdin = ZiskStdin::new();
    stdin.write(&inputs);
    println!(
        "Input prepared: {} blobs, {} bytes blob_data, {} entry_auths, prevRoot={:02x?}",
        inputs.blobs.len(),
        inputs.blob_data.len(),
        inputs.entry_auths.len(),
        inputs.prev_root,
    );

    let handle = client.execute(&PROGRAM, stdin.clone()).run()?;
    let result = handle.await?;
    println!(
        "ZisK has executed program with {} cycles in {:?} ms",
        result.get_execution_steps(),
        result.get_execution_time(),
    );

    let prove_handle = client.prove(&PROGRAM, stdin.clone()).run()?;
    let vadcop_result = prove_handle.await?;
    let vkey = PROGRAM.vk()?;
    vadcop_result.with_program_vk(&vkey).verify()?;

    println!("successfully generated and verified proof for the program!");
    Ok(())
}

/// Build the [`GuestInputs`] the prover hands to the guest.
///
/// Default: `common::sample_inputs()` — a synthetic but fully valid blob with
/// two Anvil-signed entries. Useful for local development.
///
/// Real-blob mode: when all three of `L1_RPC`, `L1_BEACON_RPC`, and
/// `BLOB_REFS` are set, the host fetches real on-chain blobs through
/// `blob-sync` (which KZG-batch-verifies them against the trusted setup
/// before they reach this function) and substitutes their bytes for the
/// synthetic ones. `expected_public_inputs_hash` is recomputed from the new
/// `blobs`. `entry_auths` continues to come from `sample_inputs()`; replace
/// it with auths matching the entries actually inside the fetched blob when
/// proving against blobs your system produced — the guest enforces
/// `entry_auths.len() == decoded_entry_count` and verifies each signature.
async fn build_inputs() -> Result<GuestInputs> {
    let (l1_rpc, beacon_rpc, blob_refs_env) = match (
        std::env::var("L1_RPC").ok(),
        std::env::var("L1_BEACON_RPC").ok(),
        std::env::var("BLOB_REFS").ok(),
    ) {
        (Some(l), Some(b), Some(r)) => (l, b, r),
        _ => {
            println!(
                "[host] L1_RPC / L1_BEACON_RPC / BLOB_REFS not all set — \
                 running with synthetic sample_inputs()"
            );
            return Ok(sample_inputs());
        }
    };

    let refs = parse_blob_refs(&blob_refs_env)?;
    println!(
        "[host] fetching {} blob(s) via blob-sync (with KZG batch verification)",
        refs.len(),
    );
    let cfg = FetchConfig::new(
        Url::parse(&l1_rpc).context("parsing L1_RPC")?,
        Url::parse(&beacon_rpc).context("parsing L1_BEACON_RPC")?,
    );
    let blob_data = blob_sync::fetch_blob_data(&cfg, &refs)
        .await
        .context("blob_sync::fetch_blob_data failed")?;
    println!(
        "[host] fetched {} bytes — KZG batch verification passed",
        blob_data.len(),
    );

    let mut inputs = sample_inputs();
    inputs.blobs = refs;
    inputs.blob_data = blob_data;
    inputs.expected_public_inputs_hash = compute_public_inputs_hash(&inputs);
    Ok(inputs)
}

/// Parse `BLOB_REFS` (`"block:tx_index,block:tx_index,..."`) into the
/// concrete refs that `blob-sync` and the guest both consume.
fn parse_blob_refs(s: &str) -> Result<Vec<BlobRefIn>> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (block, tx) = pair.split_once(':').with_context(|| {
                format!("BLOB_REFS entry '{pair}' missing ':' (expected block:tx_index)")
            })?;
            Ok(BlobRefIn {
                block_number: block.trim().parse().context("parsing block_number")?,
                tx_index: tx.trim().parse().context("parsing tx_index")?,
            })
        })
        .collect()
}
