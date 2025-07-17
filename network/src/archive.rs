use anyhow::{anyhow, Result};
use solana_transaction_status_client_types::TransactionDetails;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use tokio::time::{sleep, Duration};
use tape_client::{get_slot, get_blocks_with_limit, get_block_by_number, get_archive_account};
use tape_client::utils::{process_block, ProcessedBlock};
use reqwest::Client as HttpClient;
use serde_json::json;
use base64::decode;

use super::store::TapeStore;

/// Archive loop that continuously fetches and processes blocks from the Solana network.
pub async fn archive_loop(
    store: &TapeStore,
    client: &RpcClient,
    starting_slot: Option<u64>,
    trusted_peer: Option<String>,
) -> Result<()> {
    // If a trusted peer is provided, sync with it first
    if let Some(peer_url) = trusted_peer.clone() {
        println!("DEBUG: Using trusted peer: {}", peer_url);
        println!("DEBUG: Syncing with trusted peer");
        println!("DEBUG: This may take a while... please be patient");
        sync_with_trusted_peer(store, client, &peer_url).await?;
    }

    let interval = Duration::from_secs(2);

    let mut latest_slot = match starting_slot {
        Some(slot) => slot,
        None => {
            get_slot(client).await?
        }
    };

    println!("DEBUG: Initial slot tip: {}", latest_slot);

    // Resume from store or start at current tip
    let mut last_processed_slot = starting_slot
        .or_else(|| store.get_health().map(|(slot, _)| slot).ok())
        .unwrap_or(latest_slot);

    let mut iteration_count = 0;

    loop {
        match try_archive_iteration(
            store,
            client,
            &mut latest_slot,
            &mut last_processed_slot,
            &mut iteration_count,
        ).await {
            Ok(()) => println!("DEBUG: Block processing iteration completed successfully"),
            Err(e) => eprintln!("ERROR: Block processing iteration failed: {:?}", e),
        }

        drift_status(store, latest_slot, last_processed_slot);
        sleep(interval).await;
    }
}

/// Archive a block from the Solana network
async fn try_archive_iteration(
    store: &TapeStore,
    client: &RpcClient,
    latest_slot: &mut u64,
    last_processed_slot: &mut u64,
    iteration_count: &mut u64,
) -> Result<()> {
    *iteration_count += 1;

    // Refresh the slot tip every 10 iterations
    if *iteration_count % 10 == 0 {
        if let Ok(slot) = get_slot(client).await {
            *latest_slot = slot;
            println!("DEBUG: Updated slot tip: {}", slot);
        } else {
            println!("DEBUG: Failed to get slot tip");
        }
    }

    // Fetch up to 100 new slots starting just above what we've processed
    let start = *last_processed_slot + 1;
    let slots = get_blocks_with_limit(client, start, 100).await?;
    println!("DEBUG: Fetched {} new slots from {}", slots.len(), start);

    for slot in slots {
        let block = get_block_by_number(client, slot, TransactionDetails::Full).await?;
        let processed = process_block(block, slot)?;

        if !processed.tapes.is_empty() || !processed.writes.is_empty() {
            archive_block(store, &processed)?;
        }

        *last_processed_slot = slot;
    }

    Ok(())
}

fn archive_block(store: &TapeStore, block: &ProcessedBlock) -> Result<()> {
    for (address, number) in &block.tapes {
        store.add_tape(*number, address)?;
    }

    for ((tape, segment, _parent), data) in &block.writes {
        store.add_segment(tape, *segment, data.clone())?;
        store.add_slot(tape, *segment, block.slot)?;
        //store.add_slot(tape, *segment, parent)?;
    }

    Ok(())
}

/// Syncs all tapes up to the current archive count from a trusted peer
async fn sync_with_trusted_peer(
    store: &TapeStore,
    client: &RpcClient,
    trusted_peer_url: &str,
) -> Result<()> {
    // Fetch archive state to know how many tapes exist
    let (archive, _) = get_archive_account(client).await?;
    let total = archive.tapes_stored;
    let http = HttpClient::new();

    for tape_number in 1..(total+1) {
        // Skip if we already have this tape
        if store.get_tape_address(tape_number).is_ok() {
            continue;
        }

        // Get the tape's Solana address
        let addr_resp = http.post(trusted_peer_url)
            .header("Content-Type", "application/json")
            .body(json!({
                "jsonrpc": "2.0", "id": 1,
                "method": "getTapeAddress",
                "params": { "tape_number": tape_number }
            }).to_string())
            .send().await?
            .json::<serde_json::Value>().await?;

        //println!("DEBUG: getTapeAddress response: {:?}", addr_resp);

        let addr_str = addr_resp["result"]
            .as_str()
            .ok_or_else(|| anyhow!("Invalid getTapeAddress response: {:?}", addr_resp))?;
        let tape_address: Pubkey = addr_str.parse()?;

        // Store the tape record
        store.add_tape(tape_number, &tape_address)?;

        // Fetch all segments for this tape
        let seg_resp = http.post(trusted_peer_url)
            .header("Content-Type", "application/json")
            .body(json!({
                "jsonrpc": "2.0", "id": 4,
                "method": "getTape",
                "params": { "tape_address": addr_str }
            }).to_string())
            .send().await?
            .json::<serde_json::Value>().await?;

        println!("DEBUG: Syncing tape {}, address {}", tape_number, tape_address);

        let segments = seg_resp["result"].as_array()
            .ok_or_else(|| anyhow!("Invalid getTape response: {:?}", seg_resp))?;

        for seg in segments {
            let seg_num = seg["segment_number"]
                .as_u64()
                .ok_or_else(|| anyhow!("Invalid segment_number: {:?}", seg))?;
            let data_b64 = seg["data"]
                .as_str()
                .ok_or_else(|| anyhow!("Invalid data field: {:?}", seg))?;
            let data = decode(data_b64)?;

            store.add_segment(&tape_address, seg_num, data)?;
        }
    }

    Ok(())
}

fn drift_status(
    store: &TapeStore,
    latest_slot: u64,
    last_processed_slot: u64,
) {
    let drift = latest_slot.saturating_sub(last_processed_slot);

    // Persist updated health (last_processed_slot + drift)
    if let Err(e) = store.update_health(last_processed_slot, drift) {
        println!("ERROR: failed to write health metadata: {:?}", e);
    }

    let health_status = if drift < 50 {
        "Healthy"
    } else if drift < 200 {
        "Slightly behind"
    } else {
        "Falling behind"
    };

    println!(
        "DEBUG: Drift {} slots behind tip ({}), status: {}",
        drift, latest_slot, health_status
    );
}
