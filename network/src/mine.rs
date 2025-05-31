use anyhow::{Result, anyhow};
use chrono::Utc;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{signature::Keypair, pubkey::Pubkey};
use tape_client::mine::mine::perform_mining;
use tokio::time::{sleep, Duration};

use tape_client::utils::*;
use tape_api::prelude::*;

use brine_tree::MerkleTree;
use crankx::equix::SolverMemory;
use crankx::{
    solve_with_memory,
    Solution, 
    CrankXError
};

use super::store::TapeStore;

pub async fn mine_loop(
    store: &TapeStore, 
    client: &RpcClient, 
    miner_address: &Pubkey,
    signer: &Keypair,
) -> Result<()> {
    let interval = Duration::from_secs(60);

    loop {
        match try_mine_iteration(store, client, miner_address, signer).await {
            Ok(()) => println!("DEBUG: Mining iteration completed successfully"),
            Err(e) => {
                // Log the error (you can use a proper logger like `log::error!` if set up)
                eprintln!("ERROR: Mining iteration failed: {:?}", e);
            }
        }

        println!("DEBUG: Waiting for next interval...");
        sleep(interval).await;
    }
}

async fn try_mine_iteration(
    store: &TapeStore,
    client: &RpcClient,
    miner_address: &Pubkey,
    signer: &Keypair,
) -> Result<()> {
    let current_time = Utc::now().timestamp();

    println!("DEBUG: Starting mine process...");

    let epoch = get_epoch_account(client)
        .await
        .map_err(|e| anyhow!("Failed to get epoch account: {}", e))?.0;

    println!("DEBUG: Current time: {}", current_time);
    println!("DEBUG: Epoch.last_epoch_at: {}", epoch.last_epoch_at);

    if current_time > epoch.last_epoch_at + EPOCH_SECONDS {
        println!("DEBUG: Epoch is stale, advancing...");
        tape_client::advance(client, signer).await?;
        println!("DEBUG: Advanced epoch to {}", current_time);
    }

    //println!("DEBUG: Epoch account: {:?}", epoch);

    let miner = get_miner_account(client, miner_address)
        .await
        .map_err(|e| anyhow!("Failed to get miner account: {}", e))?.0;

    //println!("DEBUG: Miner account: {:?}", miner);

    let tape_number = miner.recall_tape;

    println!("DEBUG: Recall tape number: {:?}", tape_number);

    let tape_address = store.get_tape_address(tape_number);

    println!("DEBUG: Tape address: {:?}", tape_address);

    if let Ok(tape_address) = tape_address {
        let tape = get_tape_account(client, &tape_address)
            .await
            .map_err(|e| anyhow!("Failed to get tape account: {}", e))?.0;

        //println!("DEBUG: Tape account: {:?}", tape);

        let segments = store.get_tape_segments(&tape_address)?;
        if segments.len() != tape.total_segments as usize {
            return Err(anyhow!("Invalid number of segments for tape {}: expected {}, got {}", 
                tape_address, tape.total_segments, segments.len()));
        }

        let (solution, recall_segment, merkle_proof) = compute_challenge_solution(
            &tape,
            &miner,
            segments,
            epoch.difficulty,
        )?;

        let sig = perform_mining(
            client, 
            signer, 
            *miner_address, 
            tape_address, 
            solution, 
            recall_segment, 
            merkle_proof,
        ).await?;

        println!("DEBUG: Mining successful! Signature: {:?}", sig);
    } else {
        println!("DEBUG: Tape not found, continuing...");
    }

    println!("DEBUG: Catching up with primary...");
    store.catch_up_with_primary()?;

    Ok(())
}

fn compute_challenge_solution(
    tape: &Tape,
    miner: &Miner,
    segments: Vec<(u64, Vec<u8>)>,
    epoch_difficulty: u64,
) -> Result<(Solution, [u8; SEGMENT_SIZE], [[u8; 32]; TREE_HEIGHT])> {
    //println!("DEBUG: Segments: {:?}", segments);

    let segment_number = compute_recall_segment(
        &miner.current_challenge,
        tape.total_segments
    );

    let mut leaves = Vec::new();
    let mut recall_segment = [0; SEGMENT_SIZE];

    let mut merkle_tree = MerkleTree::<{TREE_HEIGHT}>::new(&[tape.merkle_seed.as_ref()]);

    for (segment_id, segment_data) in segments.iter() {
        if *segment_id == segment_number {
            recall_segment.copy_from_slice(segment_data);
        }

        // Create our canonical segment of exactly SEGMENT_SIZE bytes 
        // and compute the merkle leaf

        let data = padded_array::<SEGMENT_SIZE>(segment_data);
        let leaf = compute_leaf(
            *segment_id,
            &data,
        );

        leaves.push(leaf);

        // TODO: we don't actually need to do this, this is just for 
        // debugging and making sure the local root matches the tape root
        merkle_tree.try_add_leaf(leaf).map_err(|e| {
            anyhow!("Failed to add leaf to Merkle tree: {:?}", e)
        })?;
    }

    //println!("DEBUG: Merkle root: {:?}", merkle_tree.get_root());

    let merkle_proof = merkle_tree.get_merkle_proof(&leaves, segment_number as usize);
    let merkle_proof = merkle_proof
        .iter()
        .map(|v| v.to_bytes())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();

    if merkle_tree.get_root() != tape.merkle_root.into() {
        return Err(anyhow!("Merkle root mismatch"));
    } else {
        println!("DEBUG: Merkle root matches tape root!");
    }

    let solution = solve_challenge(
        miner.current_challenge, 
        &recall_segment, 
        epoch_difficulty
    )?;

    println!("DEBUG: Solution difficulty: {:?}", solution.difficulty());

    solution.is_valid(&miner.current_challenge, &recall_segment)
        .map_err(|_| anyhow!("Invalid solution"))?;

    println!("DEBUG: Solution is valid!");

    Ok((solution, recall_segment, merkle_proof))
}

fn solve_challenge<const N: usize>(
    challenge: [u8; 32],
    data: &[u8; N],
    difficulty: u64,
) -> Result<Solution, CrankXError> {
    let mut memory = SolverMemory::new();
    let mut nonce: u64 = 0;

    loop {
        if let Ok(solution) = solve_with_memory(&mut memory, &challenge, data, &nonce.to_le_bytes()) {
            if solution.difficulty() >= difficulty as u32 {
                return Ok(solution);
            }
        }
        nonce += 1;
    }
}
