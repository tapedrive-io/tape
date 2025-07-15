use crankx::Solution;
use brine_tree::{Leaf, verify};
use steel::*;
use tape_api::prelude::*;

const MIN_CONSISTENCY_MULTIPLIER: u64  = 1;
const MAX_CONSISTENCY_MULTIPLIER: u64  = 32;
const REWARD_SCALE_FACTOR: u64         = 16;

pub fn process_mine(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let current_time = Clock::get()?.unix_timestamp;
    let args = Mine::try_from_bytes(data)?;
    let [
        signer_info, 
        archive_info,
        epoch_info, 
        block_info,
        miner_info, 
        tape_info,
        slot_hashes_info
    ] = accounts else { 
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    signer_info.is_signer()?;

    let archive = archive_info
        .is_archive()?
        .as_account_mut::<Archive>(&tape_api::ID)?;

    let epoch = epoch_info
        .is_epoch()?
        .as_account_mut::<Epoch>(&tape_api::ID)?;

    let block = block_info
        .is_epoch()?
        .as_account_mut::<Block>(&tape_api::ID)?;

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

    let solution   = Solution::new(args.digest, args.nonce);
    let difficulty = solution.difficulty();

    check_condition(
        difficulty >= epoch.target_difficulty as u32,
        TapeError::SolutionTooEasy,
    )?;

    let miner_challenge = compute_challenge(
        &block.challenge,
        &miner.challenge,
    );

    let recall_tape = compute_recall_tape(
        &miner_challenge,
        archive.tapes_stored
    );

    check_condition(
        tape.number == recall_tape,
        TapeError::SolutionInvalid,
    )?;

    let segment_number = compute_recall_segment(
        &miner.challenge, 
        tape.total_segments
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

    // Verify that the PoW solution is good
    check_condition(
        solution.is_valid(&miner_challenge, &args.recall_segment).is_ok(),
        TapeError::SolutionInvalid,
    )?;

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

    // Update miner
    miner.unclaimed_rewards   += final_reward;
    miner.total_rewards       += final_reward;
    miner.total_proofs        += 1;
    miner.last_proof_at        = current_time.max(target_time);

    // Update the challenge values

    let next_miner_challenge = compute_next_challenge(
        &miner.challenge,
        slot_hashes_info
    );

    let next_block_challenge = compute_next_challenge(
        &block.challenge,
        slot_hashes_info
    );

    miner.challenge = next_miner_challenge;
    block.challenge = next_block_challenge;

    // Check if we need to advance the epoch

    if epoch.progress >= EPOCH_BLOCKS {
        advance_epoch(epoch, current_time)?;

        // Update the reward rate for the new epoch
        let storage_rate   = get_storage_rate(archive.bytes_stored);
        let inflation_rate = get_inflation_rate(epoch.number);

        epoch.reward_rate = storage_rate
            .saturating_add(inflation_rate);
    } else {

        // Epoch is still in progress, increment the progress
        epoch.progress = epoch.progress
            .saturating_add(1);
    }

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


// Helper: Advance the epoch state
#[inline(always)]
fn advance_epoch(
    epoch: &mut Epoch,
    current_time: i64,
) -> ProgramResult {

    adjust_participation(epoch);
    adjust_difficulty(epoch, current_time);

    epoch.number             = epoch.number.saturating_add(1);
    epoch.target_difficulty  = epoch.target_difficulty.max(7);
    epoch.target_unique      = epoch.target_unique.max(1);
    epoch.progress           = 0;
    epoch.duplicates         = 0;
    epoch.last_epoch_at      = current_time;

    Ok(())
}


// Every epoch, the protocol adjusts the minimum required difficulty for a block solution.
//
// Proof Difficulty:
// If blocks were solved faster than 1 minute on average, increase difficulty.
// If blocks were slower, decrease difficulty.
//
// This keeps block times near the 1-minute target.
#[inline(always)]
fn adjust_difficulty(epoch: &mut Epoch, current_time: i64) {

    let elapsed_time = current_time.saturating_sub(epoch.last_epoch_at);
    let average_time_per_block = elapsed_time / EPOCH_BLOCKS as i64;

    // If blocks were solved faster than 1 minute, increase difficulty
    if average_time_per_block < BLOCK_DURATION_SECONDS as i64 {
        epoch.target_difficulty = epoch.target_difficulty
            .saturating_add(1);

    // If they were slower, decrease difficulty
    } else {
        epoch.target_difficulty = epoch.target_difficulty
            .saturating_sub(1);

    }
}

// Every epoch, the protocol adjusts the minimum required unique proofs for a single block. This
// is referred to as the participation target.
//
// Participation Target (X):
// * If all submissions during the epoch came from unique miners, increase X by 1.
// * If any duplicates occurred (same miner submitting multiple times in a block), decrease X by 1.
//
// This helps tune how many miners can share in a block reward, balancing inclusivity and competitiveness.
#[inline(always)]
fn adjust_participation(epoch: &mut Epoch) {
    // If all miner submissions were unique, increase by 1
    if epoch.duplicates == 0 {
        epoch.target_unique = epoch.target_unique
            .saturating_add(1);

    // If there were duplicates, decrease target by 1
    } else {
        epoch.target_unique = epoch.target_unique
            .saturating_sub(1);
    }
}

// Pre-computed archive reward rate based on current bytes stored. This is calculated such that
// each block is worth 1 minute of a 100 year time horizon, with the write cost being
// 1 tape per megabyte stored. The hard-coded values avoid u128 math for simplicity and CU.
//
// Reward per minute = (total_bytes_stored) / (total_minutes_in_100_years × bytes_per_tape)
// Equation: reward_per_minute = bytes / (100 * 365 * 24 * 60 * (1 MiB / TAPE))
#[inline(always)]
fn get_storage_rate(archive_byte_size: u64) -> u64 {
    match archive_byte_size {
        n if n < 1000              => 0,            // ~ roughly no storage, no reward
        n if n < 1048576           => 190,          // 1.0 MiB      ~ 0.00000002  TAPE/min
        n if n < 2486565           => 451,          // 2.4 MiB      ~ 0.00000005  TAPE/min
        n if n < 5896576           => 1070,         // 5.6 MiB      ~ 0.00000011  TAPE/min
        n if n < 13982985          => 2537,         // 13.3 MiB     ~ 0.00000025  TAPE/min
        n if n < 33158884          => 6017,         // 31.6 MiB     ~ 0.00000060  TAPE/min
        n if n < 78632107          => 14267,        // 75.0 MiB     ~ 0.00000143  TAPE/min
        n if n < 186466111         => 33833,        // 177.8 MiB    ~ 0.00000338  TAPE/min
        n if n < 442180832         => 80231,        // 421.7 MiB    ~ 0.00000802  TAPE/min
        n if n < 1048575999        => 190259,       // 1000.0 MiB   ~ 0.00001903  TAPE/min
        n if n < 2486565554        => 451175,       // 2.3 GiB      ~ 0.00004512  TAPE/min
        n if n < 5896576174        => 1069904,      // 5.5 GiB      ~ 0.00010699  TAPE/min
        n if n < 13982985692       => 2537141,      // 13.0 GiB     ~ 0.00025371  TAPE/min
        n if n < 33158884597       => 6016510,      // 30.9 GiB     ~ 0.00060165  TAPE/min
        n if n < 78632107044       => 14267394,     // 73.2 GiB     ~ 0.00142674  TAPE/min
        n if n < 186466111066      => 33833322,     // 173.7 GiB    ~ 0.00338333  TAPE/min
        n if n < 442180832779      => 80231450,     // 411.8 GiB    ~ 0.00802315  TAPE/min
        n if n < 1048575999999     => 190258752,    // 976.6 GiB    ~ 0.01902588  TAPE/min
        n if n < 2486565554787     => 451174602,    // 2.3 TiB      ~ 0.04511746  TAPE/min
        n if n < 5896576174027     => 1069903587,   // 5.4 TiB      ~ 0.10699036  TAPE/min
        n if n < 13982985692520    => 2537141233,   // 12.7 TiB     ~ 0.25371412  TAPE/min
        n if n < 33158884597887    => 6016510008,   // 30.2 TiB     ~ 0.60165100  TAPE/min
        n if n < 78632107044498    => 14267393633,  // 71.5 TiB     ~ 1.42673936  TAPE/min
        n if n < 186466111066097   => 33833322109,  // 169.6 TiB    ~ 3.38333221  TAPE/min
        n if n < 442180832779129   => 80231450424,  // 402.2 TiB    ~ 8.02314504  TAPE/min
        n if n < 1048576000000000  => 190258751903, // 953.7 TiB    ~ 19.02587519 TAPE/min
        _ => 20,                                    // +1.0 PiB     ~ 20.00000000 TAPE/min
    }
}

// Pre-computed inflation rate based on current epoch number. Decay of ~15% every 12 months with a
// target of 2.1 million TAPE worth of total inflation over 25 years. After which, the archive
// storage fees would take over, with no further inflation.
#[inline(always)]
fn get_inflation_rate(current_epoch: u64) -> u64 {
    match current_epoch {
        n if n < 1 * EPOCHS_PER_YEAR   => 10000000000, // Year ~1,  about 1.00 TAPE/min
        n if n < 2 * EPOCHS_PER_YEAR   => 7500000000,  // Year ~2,  about 0.75 TAPE/min
        n if n < 3 * EPOCHS_PER_YEAR   => 5625000000,  // Year ~3,  about 0.56 TAPE/min
        n if n < 4 * EPOCHS_PER_YEAR   => 4218750000,  // Year ~4,  about 0.42 TAPE/min
        n if n < 5 * EPOCHS_PER_YEAR   => 3164062500,  // Year ~5,  about 0.32 TAPE/min
        n if n < 6 * EPOCHS_PER_YEAR   => 2373046875,  // Year ~6,  about 0.24 TAPE/min
        n if n < 7 * EPOCHS_PER_YEAR   => 1779785156,  // Year ~7,  about 0.18 TAPE/min
        n if n < 8 * EPOCHS_PER_YEAR   => 1334838867,  // Year ~8,  about 0.13 TAPE/min
        n if n < 9 * EPOCHS_PER_YEAR   => 1001129150,  // Year ~9,  about 0.10 TAPE/min
        n if n < 10 * EPOCHS_PER_YEAR  => 750846862,   // Year ~10, about 0.08 TAPE/min
        n if n < 11 * EPOCHS_PER_YEAR  => 563135147,   // Year ~11, about 0.06 TAPE/min
        n if n < 12 * EPOCHS_PER_YEAR  => 422351360,   // Year ~12, about 0.04 TAPE/min
        n if n < 13 * EPOCHS_PER_YEAR  => 316763520,   // Year ~13, about 0.03 TAPE/min
        n if n < 14 * EPOCHS_PER_YEAR  => 237572640,   // Year ~14, about 0.02 TAPE/min
        n if n < 15 * EPOCHS_PER_YEAR  => 178179480,   // Year ~15, about 0.02 TAPE/min
        n if n < 16 * EPOCHS_PER_YEAR  => 133634610,   // Year ~16, about 0.01 TAPE/min
        n if n < 17 * EPOCHS_PER_YEAR  => 100225957,   // Year ~17, about 0.01 TAPE/min
        n if n < 18 * EPOCHS_PER_YEAR  => 75169468,    // Year ~18, about 0.01 TAPE/min
        n if n < 19 * EPOCHS_PER_YEAR  => 56377101,    // Year ~19, about 0.01 TAPE/min
        n if n < 20 * EPOCHS_PER_YEAR  => 42282825,    // Year ~20, about 0.00 TAPE/min
        n if n < 21 * EPOCHS_PER_YEAR  => 31712119,    // Year ~21, about 0.00 TAPE/min
        n if n < 22 * EPOCHS_PER_YEAR  => 23784089,    // Year ~22, about 0.00 TAPE/min
        n if n < 23 * EPOCHS_PER_YEAR  => 17838067,    // Year ~23, about 0.00 TAPE/min
        n if n < 24 * EPOCHS_PER_YEAR  => 13378550,    // Year ~24, about 0.00 TAPE/min
        n if n < 25 * EPOCHS_PER_YEAR  => 10033913,    // Year ~25, about 0.00 TAPE/min
        _ => 0,
    }
}
