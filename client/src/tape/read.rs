use anyhow::{Result, anyhow};
use solana_transaction_status_client_types::TransactionDetails;
use solana_sdk::{signature::Signature, pubkey::Pubkey};
use tape_api::prelude::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::collections::{BinaryHeap, HashMap, HashSet};
use crate::{utils::*, consts::*};

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

        //println!("Processing slot: {}", current_slot);

        let block = get_block_by_number(client, current_slot, TransactionDetails::Full).await?;
        let processed = process_block(block, current_slot)?;

        let mut parents: HashSet<u64> = HashSet::new();

        for ((tape, segment, parent), data) in &processed.writes {
            if tape != tape_address {
                continue;
            }

            // Insert only if not present (most recent first due to priority queue)
            segments.entry(*segment).or_insert(data.clone());

            if *parent != 0 {
                if *parent > current_slot {
                    //println!("Found parent slot: {}", parent);
                    //println!("Current slot: {}", current_slot);

                    return Err(anyhow::anyhow!("Parent slot must be earlier than current"));
                }
                parents.insert(*parent);
            }
        }

        for p in parents {
            queue.push(p);
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

    // println!("Read {} segments from tape {}", segments.len(), tape_address);
    // println!("Total data size: {} bytes", output.len());
    // println!("Read complete");
    // println!("data: {:?}", output);

    Ok(output)
}



pub async fn read_from_tape(
    client: &RpcClient,
    signature: &Signature,
) -> Result<Vec<u8>> {
    let tx = get_transaction_with_retry(client, signature, MAX_RETRIES).await?;

    let instruction = tx
        .message
        .instructions()
        .into_iter()
        .find(|ix| *ix.program_id(&tx.message.static_account_keys()) == tape_api::ID)
        .ok_or_else(|| anyhow!("No tape instruction found in chunk: {}", signature))?;

    let ix_type = InstructionType::try_from(instruction.data[0])
        .map_err(|_| anyhow!("Invalid instruction type"))?;

    match ix_type {
        InstructionType::Update => {
            // Updates are a bit more complex and not implemented yet. If you need this, use
            // TAPENET to fetch the entire tape in one go.

            todo!("Update instruction not implemented yet");
        }
        InstructionType::Write => {
            Ok(instruction.data[1..].to_vec())
        }
        _ => Err(anyhow!("Unexpected instruction type: {:?}", ix_type)),
    }
}


