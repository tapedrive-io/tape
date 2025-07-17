use anyhow::{Result, anyhow};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status_client_types::TransactionDetails;
use std::collections::{BinaryHeap, HashMap, HashSet};
use tape_api::prelude::*;
use crate::utils::*;

pub async fn read_from_block(
    client: &RpcClient,
    tape_address: &Pubkey,
    slot: u64,
) -> Result<Vec<u8>> {

    let mut segments: HashMap<u64, Vec<u8>> = HashMap::new();
    let mut visited: HashSet<u64> = HashSet::new();
    let mut queue: BinaryHeap<u64> = BinaryHeap::new();

    queue.push(slot);

    while let Some(current_slot) = queue.pop() {
        if !visited.insert(current_slot) {
            continue; // Skip if already visited
        }

        let block = get_block_by_number(client, current_slot, TransactionDetails::Full).await?;
        let processed = process_block(block, current_slot)?;

        let mut parents: HashSet<u64> = HashSet::new();

        for (key, data) in &processed.segment_writes {
            if key.address != *tape_address {
                continue;
            }

            // Insert only if not present (prefers most recent due to processing newer slots first)
            segments.entry(key.segment_number).or_insert(data.clone());

            if key.slot != 0 {
                if key.slot > current_slot {
                    return Err(anyhow!("Parent slot must be earlier than current"));
                }

                parents.insert(key.slot);
            }
        }

        for parent in parents {
            queue.push(parent);
        }
    }

    // Sort segments by key and concatenate data
    let mut keys: Vec<u64> = segments.keys().cloned().collect();
    keys.sort();

    let mut output = Vec::new();
    for key in keys {
        let segment = padded_array::<SEGMENT_SIZE>(&segments[&key]);
        output.extend_from_slice(&segment);
    }

    Ok(output)
}
