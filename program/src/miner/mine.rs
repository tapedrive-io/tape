use crankx::Solution;
use brine_tree::{Leaf, verify};
use steel::*;
use tape_api::prelude::*;

use super::compute_challenge;

const MIN_CONSISTENCY_MULTIPLIER: u64  = 1;
const MAX_CONSISTENCY_MULTIPLIER: u64  = 32;
const REWARD_SCALE_FACTOR: u64         = 16;

pub fn process_mine(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let current_time = Clock::get()?.unix_timestamp;
    let args = Mine::try_from_bytes(data)?;
    let [
        signer_info, 
        spool_info, 
        miner_info, 
        tape_info,
        epoch_info, 
        archive_info,
        slot_hashes_info
    ] = accounts else { 
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    signer_info.is_signer()?;

    let epoch = epoch_info
        .is_epoch()?
        .as_account::<Epoch>(&tape_api::ID)?;

    let archive = archive_info
        .is_archive()?
        .as_account_mut::<Archive>(&tape_api::ID)?;

    let spool = spool_info
        .is_spool()?
        .as_account_mut::<Spool>(&tape_api::ID)?;

    let tape = tape_info
        .as_account::<Tape>(&tape_api::ID)?;

    let miner = miner_info
        .as_account_mut::<Miner>(&tape_api::ID)?;

    let (miner_address, _miner_bump) = miner_pda(miner.authority, miner.name);

    check_condition(
        miner_info.key.eq(&miner_address),
        ProgramError::InvalidSeeds
    )?;

    check_condition(
        signer_info.key.eq(&miner.authority),
        ProgramError::InvalidAccountOwner,
    )?;

    slot_hashes_info.is_sysvar(&sysvar::slot_hashes::ID)?;

    check_condition(
        current_time > epoch.last_epoch_at + EPOCH_DURATION_MINUTES,
        TapeError::StaleEpoch,
    )?;

    let target_time = miner.last_proof_at + ONE_MINUTE;
    let early_threshold = target_time - GRACE_PERIOD_SECONDS;

    check_condition(
        current_time > early_threshold,
        TapeError::SolutionTooEarly,
    )?;

    let solution   = Solution::new(args.digest, args.nonce);
    let difficulty = solution.difficulty();

    check_condition(
        difficulty >= epoch.difficulty as u32,
        TapeError::SolutionTooEasy,
    )?;

    check_condition(
        tape.number == miner.recall_tape,
        TapeError::SolutionInvalid,
    )?;

    let segment_number = compute_recall_segment(
        &miner.current_challenge, 
        tape.total_segments
    );

    solana_program::msg!(
        "Recall tape: {}",
        tape.number
    );

    solana_program::msg!(
        "Recall segment: {}",
        segment_number
    );

    // Validate that the miner actually has the data for the tape

    let merkle_root = tape.merkle_root;
    let merkle_proof = &args.recall_proof;
    let leaf = Leaf::new(&[
        (segment_number as u64).to_le_bytes().as_ref(),
        args.recall_segment.as_ref(),
    ]);

    assert!(merkle_proof.len() == PROOF_LEN as usize);

    check_condition(
        verify(
            merkle_root,
            merkle_proof,
            leaf
        ),
        TapeError::SolutionInvalid,
    )?;

    solana_program::msg!(
        "Recall proof is valid",
    );

    // Verify that the PoW solution is good
    check_condition(
        solution.is_valid(&miner.current_challenge, &args.recall_segment).is_ok(),
        TapeError::SolutionInvalid,
    )?;

    solana_program::msg!(
        "Miner solved PoW with difficulty {}, and nonce {:?}, and digest {:?}",
        difficulty,
        args.nonce,
        args.digest
    );

    // Update miner multiplier
    update_miner_multiplier(miner, current_time);

    // Calculate reward
    let base_reward = compute_base_reward(
        miner.multiplier,
        epoch.base_rate,
        epoch.difficulty,
        difficulty,
    );

    let penalized_reward = penalize_lateness(
        base_reward, 
        miner.last_proof_at, 
        current_time
    );

    let final_reward = compute_final_reward(
        penalized_reward,
        spool.available_rewards,
        epoch.target_rate,
    );

    solana_program::msg!(
        "Miner reward: {}",
        final_reward
    );

    // Update spool
    spool.theoretical_rewards += penalized_reward;
    spool.available_rewards   -= final_reward;

    // Update miner
    miner.unclaimed_rewards   += final_reward;
    miner.total_rewards       += final_reward;
    miner.total_proofs        += 1;
    miner.last_proof_at        = current_time.max(target_time);

    // Calculate the next challenge and recall tape
    let next_challenge = compute_challenge(
        &miner.current_challenge,
        slot_hashes_info
    );

    let recall_tape_number = compute_recall_tape(
        &next_challenge,
        archive.tapes_stored
    );

    miner.current_challenge = next_challenge;
    miner.recall_tape = recall_tape_number;

    solana_program::msg!(
        "Miner has {} unclaimed rewards",
        miner.unclaimed_rewards
    );

    solana_program::msg!(
        "Next recall tape number: {}",
        miner.recall_tape
    );

    Ok(())
}

// Helper: Update miner multiplier based on timing of this solution.
//
// Miners that consistently submit solutions on-time will have a higher multiplier number. This
// number is the base of pow(), which is used to calculate the base reward. Initially, the
// multiplier is set to 1, meaning the reward earned for a solution is low. As the miner submits
// more solutions, the multiplier increases, and the reward increases as well. It is in the miner's
// best interest to submit solutions on-time, as the multiplier will decrease if they are late.
//
// This encourages miners to come up with strategies that allow them quick access to the tape data
// needed to solve the challenge.
#[inline(always)]
fn update_miner_multiplier(miner: &mut Miner, current_time: i64) {
    let target_time = miner.last_proof_at + ONE_MINUTE;
    let liveness_threshold = target_time + GRACE_PERIOD_SECONDS;

    if current_time > liveness_threshold {
        // Maybe this should be a division instead of a subtraction?
        miner.multiplier = miner.multiplier
            .saturating_sub(1)
            .max(MIN_CONSISTENCY_MULTIPLIER);
    } else {
        miner.multiplier = miner.multiplier
            .saturating_add(1)
            .min(MAX_CONSISTENCY_MULTIPLIER);
    }
}

// Helper: Calculate base reward based on difficulty and multiplier
#[inline(always)]
fn compute_base_reward(
    multiplier: u64,
    base_rate: u64,
    difficulty: u64,
    difficulty_attempt: u32,
) -> u64 {
    assert!(difficulty_attempt >= difficulty as u32);

    let consistency_multiplier = multiplier
        .saturating_div(REWARD_SCALE_FACTOR)
        .max(MIN_CONSISTENCY_MULTIPLIER)
        .min(MAX_CONSISTENCY_MULTIPLIER);

    let difficulty_excess = difficulty_attempt - difficulty as u32;

    consistency_multiplier
        .saturating_pow(difficulty_excess)
        .saturating_mul(base_rate)
}

// Helper: Apply lateness penalty to base reward
//
// Following what ORE has done, we penalize the reward based on how late the miner is, the timings
// are different, but the formula is the same.
//
// Example: If late by 90 seconds (1 minute + 30 seconds), the reward was first halved for the 
// 1 minute, then further reduced for the 30 seconds.
//
// Reference:
// https://github.com/regolith-labs/ore/blob/c18503d0ee98b8a7823b993b38823b7867059659/program/src/mine.rs#L105-L125
#[inline(always)]
fn penalize_lateness(
    base_reward: u64, 
    last_proof_at: i64, 
    current_time: i64
) -> u64 {
    let target_time = last_proof_at + ONE_MINUTE;
    let liveness_threshold = target_time + GRACE_PERIOD_SECONDS;
    let mut penalized_reward = base_reward;

    if current_time > liveness_threshold {
        let late_seconds = current_time.saturating_sub(target_time) as u64;
        let late_minutes = late_seconds.saturating_div(ONE_MINUTE as u64);

        // An exponential penalty for full minutes late: base_reward / (2^late_minutes).
        if late_minutes > 0 {
            penalized_reward =
                base_reward.saturating_div(2u64.saturating_pow(late_minutes as u32));
        }

        // A linear penalty for any extra seconds: A fraction of the reward is subtracted based on the
        // proportion of extra_seconds relative to ONE_MINUTE. 
        let extra_seconds = late_seconds.saturating_sub(late_minutes.saturating_mul(ONE_MINUTE as u64));
        if extra_seconds > 0 && penalized_reward > 0 {
            let time_penalty = penalized_reward
                .saturating_div(2)
                .saturating_mul(extra_seconds)
                .saturating_div(ONE_MINUTE as u64);
            penalized_reward = penalized_reward.saturating_sub(time_penalty);
        }
    }

    penalized_reward
}

// Helper: Calculate final reward with limits
#[inline(always)]
fn compute_final_reward(
    penalized_reward: u64,
    theoretical_rewards: u64,
    target_rate: u64,
) -> u64 {
    let final_reward = penalized_reward
        .min(theoretical_rewards)
        .min(target_rate);
    final_reward
}
