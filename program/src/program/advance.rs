use tape_api::prelude::*;
use steel::*;

pub fn process_advance(accounts: &[AccountInfo<'_>], _data: &[u8]) -> ProgramResult {
    let current_time = Clock::get()?.unix_timestamp;
    let [
        signer_info, 
        archive_info,
        epoch_info, 
        block_info,
        mint_info, 
        treasury_info, 
        treasury_ata_info, 
        token_program_info
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

    let mint = mint_info
        .has_address(&MINT_ADDRESS)?
        .is_writable()?
        .as_mint()?;

    treasury_info.is_treasury()?.is_writable()?;
    treasury_ata_info.is_treasury_ata()?.is_writable()?;
    token_program_info.is_program(&spl_token::ID)?;

    if epoch.progress < EPOCH_BLOCKS {
        advance_epoch(epoch, current_time)?;

        let mint_supply    = mint.supply();
        let storage_rate   = get_storage_rate(archive.bytes_stored);
        let inflation_rate = get_inflation_rate(mint_supply);

        // Add inflation to the treasury if it doesn't exceed the max supply
        if mint_supply.saturating_add(inflation_rate) < MAX_SUPPLY {
            mint_to_signed(
                mint_info,
                treasury_ata_info,
                treasury_info,
                token_program_info,
                inflation_rate,
                &[TREASURY],
            )?;

            epoch.reward_rate = storage_rate
                .saturating_add(inflation_rate);
        } else {
            epoch.reward_rate = storage_rate;
        }

    } else {
        epoch.progress = epoch.progress.saturating_add(1);
    }




    Ok(())
}


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


/// Every epoch, the protocol adjusts the minimum required difficulty for a block solution.
///
/// Proof Difficulty:
/// If blocks were solved faster than 1 minute on average, increase difficulty.
/// If blocks were slower, decrease difficulty.
///
/// This keeps block times near the 1-minute target.
#[inline(always)]
fn adjust_difficulty(epoch: &mut Epoch, current_time: i64) {

    let elapsed_time = current_time - epoch.last_epoch_at;
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

/// Every epoch, the protocol adjusts the minimum required unique proofs for a single block. This
/// is referred to as the participation target.
///
/// Participation Target (X):
/// * If all submissions during the epoch came from unique miners, increase X by 1.
/// * If any duplicates occurred (same miner submitting multiple times in a block), decrease X by 1.
///
/// This helps tune how many miners can share in a block reward, balancing inclusivity and competitiveness.
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


/// Every epoch, the protocol adjusts the reward rate for miners based on how many bytes are
/// currently stored in the archive.
///
/// The reward rate is calculated such that each block is worth 1 minute of a 100 year time
/// horizon. Additionally, we define the write cost to be 1 tape per megabyte stored.
#[inline(always)]
fn adjust_reward_rate(archive: &mut Archive, epoch: &Epoch) {
    // If the archive is empty, no rewards are available
    if archive.bytes_stored == 0 {
        return;
    }


    // Calculate the reward rate based on the total bytes stored in the archive



    // Update the archive's reward rate
    archive.reward_rate = reward_rate;
}


/// Pre-computed archive reward rate based on current bytes stored. This is calculated such that
/// each block is worth 1 minute of a 100 year time horizon, with the write cost being
/// 1 tape per megabyte stored. The hard-coded values avoid u128 math for simplicity and CU.
///
/// Reward per minute = (total_bytes_stored) / (total_minutes_in_100_years × bytes_per_tape)
/// Equation: reward_per_minute = bytes / (100 * 365 * 24 * 60 * (1 MiB / TAPE))
#[inline(always)]
pub fn get_storage_rate(archive_byte_size: u64) -> u64 {
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

/// Pre-computed inflation rate based on current supply. Decay of ~15% every 12 months with a target
/// of 2.1 million TAPE worth of total inflation over 25 years. After which, the archive storage
/// fees would take over, with no further inflation.
#[inline(always)]
pub fn get_inflation_rate(current_supply: u64) -> u64 {
    match current_supply {
        n if n < ONE_TAPE * 525600 => 10000000000, // Year ~1,  about 1.00 TAPE/min
        n if n < ONE_TAPE * 919800 => 7500000000,  // Year ~2,  about 0.75 TAPE/min
        n if n < ONE_TAPE * 1215450 => 5625000000, // Year ~3,  about 0.56 TAPE/min
        n if n < ONE_TAPE * 1437187 => 4218750000, // Year ~4,  about 0.42 TAPE/min
        n if n < ONE_TAPE * 1603490 => 3164062500, // Year ~5,  about 0.32 TAPE/min
        n if n < ONE_TAPE * 1728217 => 2373046875, // Year ~6,  about 0.24 TAPE/min
        n if n < ONE_TAPE * 1821763 => 1779785156, // Year ~7,  about 0.18 TAPE/min
        n if n < ONE_TAPE * 1891922 => 1334838867, // Year ~8,  about 0.13 TAPE/min
        n if n < ONE_TAPE * 1944541 => 1001129150, // Year ~9,  about 0.10 TAPE/min
        n if n < ONE_TAPE * 1984006 => 750846862,  // Year ~10, about 0.08 TAPE/min
        n if n < ONE_TAPE * 2013604 => 563135147,  // Year ~11, about 0.06 TAPE/min
        n if n < ONE_TAPE * 2035803 => 422351360,  // Year ~12, about 0.04 TAPE/min
        n if n < ONE_TAPE * 2052452 => 316763520,  // Year ~13, about 0.03 TAPE/min
        n if n < ONE_TAPE * 2064939 => 237572640,  // Year ~14, about 0.02 TAPE/min
        n if n < ONE_TAPE * 2074304 => 178179480,  // Year ~15, about 0.02 TAPE/min
        n if n < ONE_TAPE * 2081328 => 133634610,  // Year ~16, about 0.01 TAPE/min
        n if n < ONE_TAPE * 2086596 => 100225957,  // Year ~17, about 0.01 TAPE/min
        n if n < ONE_TAPE * 2090547 => 75169468,   // Year ~18, about 0.01 TAPE/min
        n if n < ONE_TAPE * 2093510 => 56377101,   // Year ~19, about 0.01 TAPE/min
        n if n < ONE_TAPE * 2095732 => 42282825,   // Year ~20, about 0.00 TAPE/min
        n if n < ONE_TAPE * 2097399 => 31712119,   // Year ~21, about 0.00 TAPE/min
        n if n < ONE_TAPE * 2098649 => 23784089,   // Year ~22, about 0.00 TAPE/min
        n if n < ONE_TAPE * 2099587 => 17838067,   // Year ~23, about 0.00 TAPE/min
        n if n < ONE_TAPE * 2100000 => 13378550,   // Year ~24, about 0.00 TAPE/min
        _ => 0,
    }
}
