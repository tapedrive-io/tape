use tape_api::prelude::*;
use steel::*;

const LOW_REWARD_THRESHOLD: u64  = 32;
const HIGH_REWARD_THRESHOLD: u64 = 256;
const SMOOTHING_FACTOR: u64      = 2;

pub fn process_advance(accounts: &[AccountInfo<'_>], _data: &[u8]) -> ProgramResult {
    let current_time = Clock::get()?.unix_timestamp;
    let [
        signer_info, 
        spool_0_info, 
        spool_1_info, 
        spool_2_info, 
        spool_3_info, 
        spool_4_info, 
        spool_5_info, 
        spool_6_info, 
        spool_7_info, 
        epoch_info, 
        mint_info, 
        treasury_info, 
        treasury_ata_info, 
        token_program_info
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    signer_info.is_signer()?;

    let spool_0 = spool_0_info
        .as_account_mut::<Spool>(&tape_api::ID)?
        .assert_mut(|s| s.id == 0)?;
    let spool_1 = spool_1_info
        .as_account_mut::<Spool>(&tape_api::ID)?
        .assert_mut(|s| s.id == 1)?;
    let spool_2 = spool_2_info
        .as_account_mut::<Spool>(&tape_api::ID)?
        .assert_mut(|s| s.id == 2)?;
    let spool_3 = spool_3_info
        .as_account_mut::<Spool>(&tape_api::ID)?
        .assert_mut(|s| s.id == 3)?;
    let spool_4 = spool_4_info
        .as_account_mut::<Spool>(&tape_api::ID)?
        .assert_mut(|s| s.id == 4)?;
    let spool_5 = spool_5_info
        .as_account_mut::<Spool>(&tape_api::ID)?
        .assert_mut(|s| s.id == 5)?;
    let spool_6 = spool_6_info
        .as_account_mut::<Spool>(&tape_api::ID)?
        .assert_mut(|s| s.id == 6)?;
    let spool_7 = spool_7_info
        .as_account_mut::<Spool>(&tape_api::ID)?
        .assert_mut(|s| s.id == 7)?;

    let spools = [spool_0, spool_1, spool_2, spool_3, spool_4, spool_5, spool_6, spool_7];

    let epoch = epoch_info
        .is_epoch()?
        .as_account_mut::<Epoch>(&tape_api::ID)?;

    let mint = mint_info
        .has_address(&MINT_ADDRESS)?
        .is_writable()?
        .as_mint()?;

    treasury_info.is_treasury()?.is_writable()?;
    treasury_ata_info.is_treasury_ata()?.is_writable()?;
    token_program_info.is_program(&spl_token::ID)?;

    // Check if the epoch is ready to be processed
    if still_active(epoch, current_time) {
        return Ok(());
    } 

    // Check if the max supply has been reached
    let mint_supply = mint.supply();
    if mint_supply >= MAX_SUPPLY {
        return Err(TapeError::MaxSupply.into());
    }

    // Adjust emissions rate (per minute)
    epoch.target_rate = get_emissions_rate(mint_supply);

    // Calculate target rewards
    let target_rewards = epoch.target_rate * EPOCH_DURATION_MINUTES as u64;

    solana_program::msg!(
        "epoch.target_rate: {}, target_rewards: {}",
        epoch.target_rate,
        target_rewards
    );

    // Process spools and calculate mint amount
    let (amount_to_mint, actual_rewards) =
        update_spools(spools, mint_supply, target_rewards);

    // Update reward rate
    epoch.base_rate = compute_new_reward_rate(
        epoch.base_rate,
        actual_rewards,
        target_rewards,
    );

    // Adjust difficulty
    adjust_difficulty(epoch);

    solana_program::msg!(
        "previous rewards: {}",
        actual_rewards,
    );

    solana_program::msg!(
        "new epoch.base_rate: {}",
        epoch.base_rate
    );

    solana_program::msg!(
        "new epoch.difficulty: {}",
        epoch.difficulty
    );

    solana_program::msg!(
        "minting: {}",
        amount_to_mint,
    );

    // Fund the treasury token account.
    mint_to_signed(
        mint_info,
        treasury_ata_info,
        treasury_info,
        token_program_info,
        amount_to_mint,
        &[TREASURY],
    )?;

    // Increment epoch number
    epoch.number += 1;
    epoch.last_epoch_at = current_time;

    Ok(())
}

// Helper: Check if the epoch is still active.
#[inline(always)]
fn still_active(epoch: &Epoch, current_time: i64) -> bool {
    epoch.last_epoch_at
        .saturating_add(EPOCH_DURATION_MINUTES)
        .gt(&current_time)
}

// Helper: Top up spools with rewards and calculate excess mint supply needed.
#[inline(always)]
fn update_spools(
    spools: [&mut Spool; SPOOL_COUNT],
    mint_supply: u64,
    target_rewards: u64,
) -> (u64, u64) {
    let mut amount_to_mint = 0u64;
    let mut available_supply = MAX_SUPPLY.saturating_sub(mint_supply);
    let mut theoretical_rewards = 0u64;

    for spool in spools {
        let spool_topup = target_rewards
            .saturating_sub(spool.available_rewards)
            .min(available_supply);

        theoretical_rewards       += spool.theoretical_rewards;
        spool.theoretical_rewards  = 0;

        available_supply          -= spool_topup;
        amount_to_mint            += spool_topup;
        spool.available_rewards   += spool_topup;
    }

    (amount_to_mint, theoretical_rewards)
}

// Helper: Adjust difficulty based on reward rate thresholds. Very much the same as what ORE does.
// Taking their lead here for consistency and to avoid any potential issues with the reward rate.
//
// Reference: https://github.com/regolith-labs/ore/blob/c18503d0ee98b8a7823b993b38823b7867059659/program/src/reset.rs#L130-L140
#[inline(always)]
fn adjust_difficulty(epoch: &mut Epoch) {
    if epoch.base_rate < LOW_REWARD_THRESHOLD {
        epoch.difficulty += 1;
        epoch.base_rate *= 2;
    }

    if epoch.base_rate >= HIGH_REWARD_THRESHOLD && epoch.difficulty > 1 {
        epoch.difficulty -= 1;
        epoch.base_rate /= 2;
    }

    epoch.difficulty = epoch.difficulty.max(7);
}

// Helper: Compute new reward rate based on current rate and epoch rewards.
//
// Formula:
// new_rate = current_rate * (target_rewards / actual_rewards)
//
// Following what ORE here to avoid footguns.
// Reference: https://github.com/regolith-labs/ore/blob/c18503d0ee98b8a7823b993b38823b7867059659/program/src/reset.rs#L146
#[inline(always)]
fn compute_new_reward_rate(
    current_rate: u64,
    actual_rewards: u64,
    target_rewards: u64,
) -> u64 {

    if actual_rewards == 0 {
        return current_rate;
    }

    let adjusted_rate = (current_rate as u128)
        .saturating_mul(target_rewards as u128)
        .saturating_div(actual_rewards as u128) as u64;

    let min_rate = current_rate.saturating_div(SMOOTHING_FACTOR);
    let max_rate = current_rate.saturating_mul(SMOOTHING_FACTOR);
    let smoothed_rate = adjusted_rate.min(max_rate).max(min_rate);

    smoothed_rate
        .max(1)
        .min(target_rewards)
}

// Pre-computed emissions rate based on current supply. Decay of ~14% every 12 months with
// a target of 7 million TAPE.
pub fn get_emissions_rate(current_supply: u64) -> u64 {
    match current_supply {
        n if n < ONE_TAPE * 1000000 => 19025875190, // Year 1: ~1.90 TAPE/min
        n if n < ONE_TAPE * 1861000 => 16381278538, // Year 2: ~1.64 TAPE/min
        n if n < ONE_TAPE * 2602321 => 14104280821, // Year 3: ~1.41 TAPE/min
        n if n < ONE_TAPE * 3240598 => 12143785787, // Year 4: ~1.21 TAPE/min
        n if n < ONE_TAPE * 3790155 => 10455799563, // Year 5: ~1.05 TAPE/min
        n if n < ONE_TAPE * 4263323 => 9002443423,  // Year 6: ~0.90 TAPE/min
        n if n < ONE_TAPE * 4670721 => 7751103787,  // Year 7: ~0.78 TAPE/min
        n if n < ONE_TAPE * 5021491 => 6673700361,  // Year 8: ~0.67 TAPE/min
        n if n < ONE_TAPE * 5323504 => 5746056011,  // Year 9: ~0.57 TAPE/min
        n if n < ONE_TAPE * 5583536 => 4947354225,  // Year 10: ~0.49 TAPE/min
        n if n < ONE_TAPE * 5807425 => 4259671988,  // Year 11: ~0.43 TAPE/min
        n if n < ONE_TAPE * 6000193 => 3667577581,  // Year 12: ~0.37 TAPE/min
        n if n < ONE_TAPE * 6166166 => 3157784298,  // Year 13: ~0.32 TAPE/min
        n if n < ONE_TAPE * 6309069 => 2718852280,  // Year 14: ~0.27 TAPE/min
        n if n < ONE_TAPE * 6432108 => 2340931813,  // Year 15: ~0.23 TAPE/min
        n if n < ONE_TAPE * 6538045 => 2015542291,  // Year 16: ~0.20 TAPE/min
        n if n < ONE_TAPE * 6629257 => 1735381912,  // Year 17: ~0.17 TAPE/min
        n if n < ONE_TAPE * 6707790 => 1494163827,  // Year 18: ~0.15 TAPE/min
        n if n < ONE_TAPE * 6775407 => 1286475055,  // Year 19: ~0.13 TAPE/min
        n if n < ONE_TAPE * 6833625 => 1107655022,  // Year 20: ~0.11 TAPE/min
        n if n < ONE_TAPE * 6883751 => 953690974,   // Year 21: ~0.10 TAPE/min
        n if n < ONE_TAPE * 6926910 => 821127928,   // Year 22: ~0.08 TAPE/min
        n if n < ONE_TAPE * 6964069 => 706991146,   // Year 23: ~0.07 TAPE/min
        n if n < ONE_TAPE * 6996064 => 608719377,   // Year 24: ~0.06 TAPE/min
        n if n < ONE_TAPE * 7000000 => 524107383,   // Year 25: ~0.05 TAPE/min
        _ => 0,
    }
}
