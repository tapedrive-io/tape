#![cfg(test)]
pub mod utils;
use utils::*;
use rand::Rng;

use solana_sdk::{
    signer::Signer,
    transaction::Transaction,
    clock::Clock,
    pubkey::Pubkey,
    signature::Keypair
};

use tape_api::prelude::*;
use litesvm::LiteSVM;

use brine_tree::MerkleTree;
use crankx::equix::SolverMemory;
use crankx::{
    solve_with_memory,
    Solution, 
    CrankXError
};

struct StoredTape {
    pubkey: Pubkey,
    segments: Vec<Vec<u8>>,
    account: Tape,
}

#[test]
fn run_integration() {
    // Setup environment
    let (mut svm, payer) = setup_environment();

    // Initialize program
    initialize_program(&mut svm, &payer);

    // Verify initial accounts
    verify_spool_accounts(&svm);
    verify_archive_account(&svm, 0);
    verify_epoch_account(&svm);
    verify_treasury_account(&svm);
    verify_mint_account(&svm);
    verify_metadata_account(&svm);
    verify_treasury_ata(&svm);

    // Create tapes
    let mut tape_db = vec![];
    let tape_count = rand::thread_rng().gen_range(1..=20);
    for tape_idx in 0..tape_count {
        create_and_verify_tape(&mut svm, &payer, tape_idx as u64, &mut tape_db);
    }

    // Verify archive account after tape creation
    verify_archive_account(&svm, tape_count as u64);

    // Advance epoch
    let time_offset = rand::thread_rng().gen_range(1..=10);
    advance_epoch(&mut svm, &payer, time_offset);

    // Register miner
    let miner_name = "miner-name";
    let miner_address = register_miner(&mut svm, &payer, miner_name);

    // Advance clock
    let mut initial_clock = svm.get_sysvar::<Clock>();
    initial_clock.unix_timestamp = initial_clock.unix_timestamp + 60;
    svm.set_sysvar::<Clock>(&initial_clock);

    // Get miner and epoch data
    let miner_account = svm.get_account(&miner_address).unwrap();
    let miner = Miner::unpack(&miner_account.data).unwrap();
    let (epoch_address, _epoch_bump) = epoch_pda();
    let epoch_account = svm.get_account(&epoch_address).unwrap();
    let epoch = Epoch::unpack(&epoch_account.data).unwrap();

    // Compute challenge solution
    let stored_tape = &tape_db[(miner.recall_tape - 1) as usize];
    let (solution, recall_segment, merkle_proof) = 
        compute_challenge_solution(stored_tape, &miner, epoch.difficulty);

    // Perform mining
    perform_mining(
        &mut svm,
        &payer,
        miner_address,
        stored_tape.pubkey,
        solution,
        recall_segment,
        merkle_proof,
    );

    // Print final state
    let account = svm.get_account(&miner_address).unwrap();
    let miner = Miner::unpack(&account.data).unwrap();

    println!("miner.balance: {:?}", miner.unclaimed_rewards);
    println!("next recall: {:?}", miner.recall_tape);
    println!("next challenge: {:?}", miner.current_challenge);
}

fn setup_environment() -> (LiteSVM, Keypair) {
    let mut svm = setup_svm();
    let payer = create_payer(&mut svm);
    (svm, payer)
}

fn initialize_program(svm: &mut LiteSVM, payer: &Keypair) {
    let payer_pk = payer.pubkey();
    let ix = build_initialize_ix(payer_pk);
    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
    let res = send_tx(svm, tx);
    assert!(res.is_ok());
}

fn verify_spool_accounts(svm: &LiteSVM) {
    for i in 0..SPOOL_COUNT as u8 {
        let (spool_address, _bump) = spool_pda(i);
        let account = svm
            .get_account(&spool_address)
            .expect("Spool account should exist");

        let spool = Spool::unpack(&account.data).expect("Failed to unpack spool account");
        assert_eq!(spool.id, i as u64);
        assert_eq!(spool.available_rewards, 0);
        assert_eq!(spool.theoretical_rewards, 0);
    }
}

fn verify_archive_account(svm: &LiteSVM, expected_tapes_stored: u64) {
    let (archive_address, _archive_bump) = archive_pda();
    let account = svm
        .get_account(&archive_address)
        .expect("Archive account should exist");
    let archive = Archive::unpack(&account.data).expect("Failed to unpack Archive account");
    assert_eq!(archive.tapes_stored, expected_tapes_stored);
}

fn verify_epoch_account(svm: &LiteSVM) {
    let (epoch_address, _epoch_bump) = epoch_pda();
    let account = svm
        .get_account(&epoch_address)
        .expect("Epoch account should exist");
    let epoch = Epoch::unpack(&account.data).expect("Failed to unpack Epoch account");
    assert_eq!(epoch.base_rate, ONE_TAPE);
    assert_eq!(epoch.last_epoch_at, 0);
    assert_eq!(epoch.difficulty, 7);
}

fn verify_treasury_account(svm: &LiteSVM) {
    let (treasury_address, _treasury_bump) = treasury_pda();
    let _treasury_account = svm
        .get_account(&treasury_address)
        .expect("Treasury account should exist");
}

fn verify_mint_account(svm: &LiteSVM) {
    let (mint_address, _mint_bump) = mint_pda();
    let mint = get_mint(svm, &mint_address);
    assert_eq!(mint.supply, 0);
    assert_eq!(mint.decimals, TOKEN_DECIMALS);
}

fn verify_metadata_account(svm: &LiteSVM) {
    let (mint_address, _mint_bump) = mint_pda();
    let (metadata_address, _metadata_bump) = metadata_pda(mint_address);
    let account = svm
        .get_account(&metadata_address)
        .expect("Metadata account should exist");
    assert!(!account.data.is_empty());
}

fn verify_treasury_ata(svm: &LiteSVM) {
    let (treasury_ata_address, _ata_bump) = treasury_ata();
    let account = svm
        .get_account(&treasury_ata_address)
        .expect("Treasury ATA should exist");
    assert!(!account.data.is_empty());
}

fn create_and_verify_tape(
    svm: &mut LiteSVM,
    payer: &Keypair,
    tape_idx: u64,
    tape_db: &mut Vec<StoredTape>,
) {
    let payer_pk = payer.pubkey();
    let tape_name = format!("tape-name-{}", tape_idx);
    let tape_header = [42; HEADER_SIZE]; // Can be anything
    let (tape_address, _tape_bump) = tape_pda(payer_pk, &to_name(&tape_name));
    let (writer_address, _writer_bump) = writer_pda(tape_address);

    // Create tape
    let blockhash = svm.latest_blockhash();
    let ix = build_create_ix(payer_pk, &tape_name, Some(tape_header));
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
    let res = send_tx(svm, tx);
    assert!(res.is_ok());

    // Verify tape account
    let account = svm.get_account(&tape_address).unwrap();
    let tape = Tape::unpack(&account.data).unwrap();
    assert_eq!(tape.authority, payer_pk);
    assert_eq!(tape.name, to_name(&tape_name));
    assert_eq!(tape.state, u64::from(TapeState::Created));
    assert_ne!(tape.merkle_seed, [0; 32]);
    assert_eq!(tape.merkle_root, [0; 32]);
    assert_eq!(tape.header, tape_header);
    assert_eq!(tape.number, 0); // (tapes get a number when finalized)

    // Verify writer account
    let account = svm.get_account(&writer_address).unwrap();
    let writer = Writer::unpack(&account.data).unwrap();
    assert_eq!(writer.tape, tape_address);

    let mut writer_tree = MerkleTree::new(&[tape.merkle_seed.as_ref()]);
    assert_eq!(writer.state, writer_tree);

    let mut stored_tape = StoredTape {
        pubkey: tape_address,
        segments: vec![],
        account: *tape,
    };

    // Write segments
    let mut total_size = 0;

    for write_index in 0..10u64 {
        let data = format!("<segment_{}_data>", write_index).into_bytes();
        total_size += data.len() as u64;

        let blockhash = svm.latest_blockhash();
        let ix = build_write_ix(payer_pk, tape_address, writer_address, &data);
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
        let res = send_tx(svm, tx.clone());
        assert!(res.is_ok());

        let segments = data.chunks(SEGMENT_SIZE);
        for (segment_number, segment) in segments.enumerate() {
            let canonical_segment = padded_array::<SEGMENT_SIZE>(segment);

            assert!(write_segment(
                &mut writer_tree,
                (stored_tape.segments.len() + segment_number) as u64,
                &canonical_segment,
            )
            .is_ok());

            // Keep track of the segments written
            stored_tape.segments.push(canonical_segment.to_vec());
        }

        // Verify writer state
        let account = svm.get_account(&writer_address).unwrap();
        let writer = Writer::unpack(&account.data).unwrap();
        assert_eq!(writer.state, writer_tree);

        // Verify tape state
        let account = svm.get_account(&tape_address).unwrap();
        let tape = Tape::unpack(&account.data).unwrap();
        assert_eq!(tape.total_segments, stored_tape.segments.len() as u64);
        assert_eq!(tape.total_size, total_size);
        assert_eq!(tape.state, u64::from(TapeState::Writing));
        assert_eq!(tape.merkle_root, writer_tree.get_root().to_bytes());
        assert_eq!(tape.header, tape_header);
    }

    // Update a segment to ensure the update instruction works
    // pick a segment to update (e.g., the first one)
    let target_segment: u64 = 0;

    // reconstruct leaves for proof
    let mut leaves = Vec::new();
    for (segment_id, segment_data) in stored_tape.segments.iter().enumerate() {
        let data_array = padded_array::<SEGMENT_SIZE>(segment_data);
        let leaf = compute_leaf(segment_id as u64, &data_array);
        leaves.push(leaf);
    }

    // compute Merkle proof for the target segment
    let merkle_proof_vec = writer_tree.get_merkle_proof(&leaves, target_segment as usize);
    let merkle_proof: [[u8; 32]; PROOF_LEN] = merkle_proof_vec
        .iter()
        .map(|v| v.to_bytes())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();

    // old_data is the current canonical segment
    let old_data_array: [u8; SEGMENT_SIZE] = stored_tape.segments[target_segment as usize]
        .clone()
        .try_into()
        .unwrap();

    // prepare new data for that segment
    let new_raw = b"<segment_0_updated>";
    let new_data_array = padded_array::<SEGMENT_SIZE>(new_raw);

    // send the update transaction
    let blockhash = svm.latest_blockhash();
    let ix = build_update_ix(
        payer_pk,
        tape_address,
        writer_address,
        target_segment,
        old_data_array,
        new_data_array,
        merkle_proof,
    );
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
    let res = send_tx(svm, tx);
    assert!(res.is_ok());

    // update the local tree to reflect the new leaf
    assert!(update_segment(
        &mut writer_tree, 
        target_segment, 
        &old_data_array,
        &new_data_array,
        &merkle_proof,
    ).is_ok());

    // update stored_tape.segments to hold the new canonical data
    stored_tape.segments[target_segment as usize] = new_data_array.to_vec();

    // Verify writer state after update
    let account = svm.get_account(&writer_address).unwrap();
    let writer = Writer::unpack(&account.data).unwrap();
    assert_eq!(writer.state, writer_tree);

    // Verify tape state after update
    let account = svm.get_account(&tape_address).unwrap();
    let tape = Tape::unpack(&account.data).unwrap();
    assert_eq!(tape.total_segments, 10);
    assert_eq!(tape.total_size, total_size);
    assert_eq!(tape.state, u64::from(TapeState::Writing));
    assert_eq!(tape.merkle_root, writer_tree.get_root().to_bytes());
    assert_eq!(tape.header, tape_header);

    // Finalize tape
    let blockhash = svm.latest_blockhash();
    let ix = build_finalize_ix(payer_pk, tape_address, writer_address, None);
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
    let res = send_tx(svm, tx.clone());
    assert!(res.is_ok());

    // Try another update, it should fail this time
    // reuse the same old_data/new_data and proof (proof is now stale but the tx should be rejected due to finalized state)
    let blockhash = svm.latest_blockhash();
    let ix = build_update_ix(
        payer_pk,
        tape_address,
        writer_address,
        target_segment,
        old_data_array,
        new_data_array,
        merkle_proof,
    );
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
    let res = send_tx(svm, tx);
    assert!(res.is_err());

    // Verify finalized tape
    let account = svm.get_account(&tape_address).unwrap();
    let tape = Tape::unpack(&account.data).unwrap();
    assert_eq!(tape.state, u64::from(TapeState::Finalized));
    assert_eq!(tape.number, tape_idx + 1);
    assert_eq!(tape.total_segments, 10);
    assert_eq!(tape.total_size, total_size);
    assert_eq!(tape.merkle_root, writer_tree.get_root().to_bytes());

    // Verify writer account is closed
    let account = svm.get_account(&writer_address).unwrap();
    assert!(account.data.is_empty());

    stored_tape.account = *tape;
    tape_db.push(stored_tape);
}

fn advance_epoch(svm: &mut LiteSVM, payer: &Keypair, time_offset: i64) {
    let mut initial_clock = svm.get_sysvar::<Clock>();
    initial_clock.unix_timestamp = initial_clock.unix_timestamp + 900 + time_offset;
    svm.set_sysvar::<Clock>(&initial_clock);

    let payer_pk = payer.pubkey();
    let blockhash = svm.latest_blockhash();
    let ix = build_advance_ix(payer_pk);
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
    let res = send_tx(svm, tx);
    assert!(res.is_ok());
}

fn register_miner(svm: &mut LiteSVM, payer: &Keypair, miner_name: &str) -> Pubkey {
    let payer_pk = payer.pubkey();
    let (miner_address, _miner_bump) = miner_pda(payer_pk, to_name(miner_name));

    let blockhash = svm.latest_blockhash();
    let ix = build_register_ix(payer_pk, miner_name);
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
    let res = send_tx(svm, tx);
    assert!(res.is_ok());

    let account = svm.get_account(&miner_address).unwrap();
    let miner = Miner::unpack(&account.data).unwrap();
    assert_eq!(miner.authority, payer_pk);
    assert_eq!(miner.name, to_name(miner_name));
    assert_eq!(miner.unclaimed_rewards, 0);

    miner_address
}

fn compute_challenge_solution(
    stored_tape: &StoredTape,
    miner: &Miner,
    epoch_difficulty: u64,
) -> (Solution, [u8; SEGMENT_SIZE], [[u8; 32]; PROOF_LEN]) {
    let segment_number = compute_recall_segment(
        &miner.current_challenge, 
        stored_tape.account.total_segments
    ) as usize;

    let mut leaves = Vec::new();
    let mut recall_segment = [0; SEGMENT_SIZE];

    for (segment_id, segment_data) in stored_tape.segments.iter().enumerate() {
        if segment_id == segment_number {
            recall_segment.copy_from_slice(segment_data);
        }

        let data = padded_array::<SEGMENT_SIZE>(segment_data);
        let leaf = compute_leaf(
            segment_id as u64,
            &data,
        );

        leaves.push(leaf);
    }

    assert_eq!(leaves.len(), stored_tape.account.total_segments as usize);

    let solution = solve_challenge(miner.current_challenge, &recall_segment, epoch_difficulty).unwrap();
    assert!(solution.is_valid(&miner.current_challenge, &recall_segment).is_ok());

    let merkle_tree = MerkleTree::<{TREE_HEIGHT}>::new(&[stored_tape.account.merkle_seed.as_ref()]);
    let merkle_proof = merkle_tree.get_merkle_proof(&leaves, segment_number);
    let merkle_proof = merkle_proof
        .iter()
        .map(|v| v.to_bytes())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();

    (solution, recall_segment, merkle_proof)
}

fn perform_mining(
    svm: &mut LiteSVM,
    payer: &Keypair,
    miner_address: Pubkey,
    tape_address: Pubkey,
    solution: Solution,
    recall_segment: [u8; SEGMENT_SIZE],
    merkle_proof: [[u8; 32]; PROOF_LEN],
) {
    let payer_pk = payer.pubkey();
    let (spool_address, _spool_bump) = spool_pda(0);

    let blockhash = svm.latest_blockhash();
    let ix = build_mine_ix(
        payer_pk,
        miner_address,
        spool_address,
        tape_address,
        solution,
        recall_segment,
        merkle_proof,
    );

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], blockhash);
    let res = send_tx(svm, tx);
    assert!(res.is_ok());

    let account = svm.get_account(&miner_address).unwrap();
    let miner = Miner::unpack(&account.data).unwrap();
    assert!(miner.unclaimed_rewards > 0);
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
