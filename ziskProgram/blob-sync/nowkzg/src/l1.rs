//! Thin alloy-provider wrapper that walks L1 blocks and pulls out blob
//! versioned-hash references — the same input op-succinct's fetcher uses to
//! drive `kona`'s `BlobProvider`.

use alloy_consensus::Transaction as _;
use alloy_eips::BlockId;
use alloy_primitives::{Address, B256};
use alloy_provider::{Provider, ProviderBuilder, RootProvider};
use alloy_rpc_types_eth::BlockTransactions;
use anyhow::{Result, anyhow};
use url::Url;

#[derive(Clone)]
pub struct L1Client {
    provider: RootProvider,
}

#[derive(Debug, Clone)]
pub struct BlobRef {
    pub tx_hash: B256,
    pub tx_from: Address,
    pub blob_index_in_tx: usize,
    pub versioned_hash: B256,
}

#[derive(Debug)]
pub struct L1BlockBlobs {
    pub block_number: u64,
    pub block_hash: B256,
    pub block_timestamp: u64,
    pub blobs: Vec<BlobRef>,
}

impl L1Client {
    pub fn new(rpc: Url) -> Self {
        let provider = ProviderBuilder::default().connect_http(rpc);
        Self { provider }
    }

    pub async fn latest_block_number(&self) -> Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }

    /// Fetch block `n` with full transactions, return every type-3 tx's blob
    /// versioned hashes (optionally filtered by sender). Empty `blobs` is fine
    /// — most L1 blocks carry zero blob txs.
    pub async fn fetch_block_blobs(
        &self,
        block_number: u64,
        batcher: Option<Address>,
    ) -> Result<L1BlockBlobs> {
        let block = self
            .provider
            .get_block(BlockId::number(block_number))
            .full()
            .await?
            .ok_or_else(|| anyhow!("L1 block {block_number} not found"))?;

        let txs = match block.transactions {
            BlockTransactions::Full(txs) => txs,
            BlockTransactions::Hashes(_) => {
                return Err(anyhow!(
                    "RPC returned tx hashes; need full bodies. Provider built without `.full()`?"
                ));
            }
            BlockTransactions::Uncle => {
                return Err(anyhow!("uncle block has no transactions"));
            }
        };

        let mut blobs = Vec::new();
        for tx in txs {
            if let Some(filter) = batcher {
                if tx.inner.signer() != filter {
                    continue;
                }
            }
            let Some(hashes) = tx.blob_versioned_hashes() else {
                continue;
            };
            if hashes.is_empty() {
                continue;
            }
            let tx_hash = *tx.inner.inner().tx_hash();
            let tx_from = tx.inner.signer();
            for (i, vh) in hashes.iter().enumerate() {
                blobs.push(BlobRef {
                    tx_hash,
                    tx_from,
                    blob_index_in_tx: i,
                    versioned_hash: *vh,
                });
            }
        }

        Ok(L1BlockBlobs {
            block_number,
            block_hash: block.header.hash,
            block_timestamp: block.header.timestamp,
            blobs,
        })
    }
}
