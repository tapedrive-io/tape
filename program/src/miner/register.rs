use tape_api::prelude::*;
use solana_program::{
    keccak::hashv, 
    slot_hashes::SlotHash
};
use steel::*;

pub fn process_register(accounts: &[AccountInfo<'_>], data: &[u8]) -> ProgramResult {
    let current_time = Clock::get()?.unix_timestamp;
    let args = Register::try_from_bytes(data)?;
    let [
        signer_info,
        miner_info,
        archive_info,
        system_program_info, 
        rent_info,
        slot_hashes_info
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    signer_info.is_signer()?;

    let archive = archive_info
        .is_archive()?
        .as_account_mut::<Archive>(&tape_api::ID)?;

    let (miner_pda, _bump) = miner_pda(*signer_info.key, args.name);

    miner_info
        .is_empty()?
        .is_writable()?
        .has_address(&miner_pda)?;

    system_program_info.is_program(&system_program::ID)?;
    rent_info.is_sysvar(&sysvar::rent::ID)?;
    slot_hashes_info.is_sysvar(&sysvar::slot_hashes::ID)?;

    // Register miner.
    create_program_account::<Miner>(
        miner_info,
        system_program_info,
        signer_info,
        &tape_api::ID,
        &[MINER, signer_info.key.as_ref(), args.name.as_ref()],
    )?;

    let miner = miner_info.as_account_mut::<Miner>(&tape_api::ID)?;

    miner.authority         = *signer_info.key;
    miner.name              = args.name;

    miner.multiplier        = 0;
    miner.last_proof_hash   = [0; 32];
    miner.last_proof_at     = current_time;
    miner.total_proofs      = 0;
    miner.total_rewards     = 0;
    miner.unclaimed_rewards = 0;

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

    Ok(())
}

// Helper: compute the next challenge.
#[inline(always)]
pub fn compute_challenge(
    current_challenge: &[u8; 32],
    slot_hashes_info: &AccountInfo,
) -> [u8; 32] {
    let slothash = &slot_hashes_info.data.borrow()
        [0..core::mem::size_of::<SlotHash>()];

    let next_challenge = hashv(&[
        current_challenge,
        slothash,
    ]).0;

    next_challenge
}
