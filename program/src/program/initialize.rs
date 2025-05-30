use tape_api::prelude::*;
use solana_program::program_pack::Pack;
use spl_token::state::Mint;
use steel::*;

const INITIAL_DIFFICULTY: u32  = 7;
const INITIAL_REWARD_RATE: u64 = ONE_TAPE;

pub fn process_initialize(accounts: &[AccountInfo<'_>], _data: &[u8]) -> ProgramResult {
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
        archive_info, 
        epoch_info, 
        metadata_info, 
        mint_info, 
        treasury_info, 
        treasury_tokens_info, 
        system_program_info, 
        token_program_info, 
        associated_token_program_info, 
        metadata_program_info, 
        rent_sysvar_info,
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    spool_0_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[SPOOL, &[0]], &tape_api::ID)?;
    spool_1_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[SPOOL, &[1]], &tape_api::ID)?;
    spool_2_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[SPOOL, &[2]], &tape_api::ID)?;
    spool_3_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[SPOOL, &[3]], &tape_api::ID)?;
    spool_4_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[SPOOL, &[4]], &tape_api::ID)?;
    spool_5_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[SPOOL, &[5]], &tape_api::ID)?;
    spool_6_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[SPOOL, &[6]], &tape_api::ID)?;
    spool_7_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[SPOOL, &[7]], &tape_api::ID)?;

    archive_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[ARCHIVE], &tape_api::ID)?;

    epoch_info
        .is_empty()?
        .is_writable()?
        .has_seeds(&[EPOCH], &tape_api::ID)?;

    // Check mint, metadata, treasury
    let (mint_address, mint_bump) = mint_pda();
    let (treasury_address, treasury_bump) = treasury_pda();
    let (metadata_address, _metadata_bump) = metadata_pda(mint_address);

    assert_eq!(mint_bump, MINT_BUMP);
    assert_eq!(treasury_bump, TREASURY_BUMP);

    mint_info
        .is_empty()?
        .is_writable()?
        .has_address(&mint_address)?;

    metadata_info
        .is_empty()?
        .is_writable()?
        .has_address(&metadata_address)?;

    treasury_info
        .is_empty()?
        .is_writable()?
        .has_address(&treasury_address)?;

    treasury_tokens_info
        .is_empty()?
        .is_writable()?;

    // Check programs and sysvars.
    system_program_info
        .is_program(&system_program::ID)?;
    token_program_info
        .is_program(&spl_token::ID)?;
    associated_token_program_info
        .is_program(&spl_associated_token_account::ID)?;

    solana_program::log::msg!("metadata: {}", metadata_program_info.key);

    metadata_program_info
        .is_program(&mpl_token_metadata::ID)?;
    rent_sysvar_info
        .is_sysvar(&sysvar::rent::ID)?;

    let spool_infos = [
        spool_0_info, 
        spool_1_info, 
        spool_2_info, 
        spool_3_info, 
        spool_4_info, 
        spool_5_info, 
        spool_6_info,
        spool_7_info,
    ];

    // Initialize spools.
    for i in 0..SPOOL_COUNT {
        create_program_account::<Spool>(
            spool_infos[i],
            system_program_info,
            signer_info,
            &tape_api::ID,
            &[SPOOL, &[i as u8]],
        )?;
        let spool = spool_infos[i].as_account_mut::<Spool>(&tape_api::ID)?;
        spool.id = i as u64;
        spool.available_rewards = 0;
        spool.theoretical_rewards = 0;
    }

    // Initialize epoch.
    create_program_account::<Epoch>(
        epoch_info,
        system_program_info,
        signer_info,
        &tape_api::ID,
        &[EPOCH],
    )?;

    let epoch = epoch_info.as_account_mut::<Epoch>(&tape_api::ID)?;

    epoch.base_rate     = INITIAL_REWARD_RATE;
    epoch.difficulty    = INITIAL_DIFFICULTY as u64;
    epoch.last_epoch_at = 0;

    create_program_account::<Archive>(
        archive_info,
        system_program_info,
        signer_info,
        &tape_api::ID,
        &[ARCHIVE],
    )?;

    let archive = archive_info.as_account_mut::<Archive>(&tape_api::ID)?;

    // No tapes have been created yet.
    archive.tapes_stored  = 0;

    // Initialize treasury.
    create_program_account::<Treasury>(
        treasury_info,
        system_program_info,
        signer_info,
        &tape_api::ID,
        &[TREASURY],
    )?;

    // Initialize mint.
    allocate_account_with_bump(
        mint_info,
        system_program_info,
        signer_info,
        Mint::LEN,
        &spl_token::ID,
        &[MINT, MINT_SEED],
        MINT_BUMP,
    )?;
    initialize_mint_signed_with_bump(
        mint_info,
        treasury_info,
        None,
        token_program_info,
        rent_sysvar_info,
        TOKEN_DECIMALS,
        &[MINT, MINT_SEED],
        MINT_BUMP,
    )?;

    // Initialize mint metadata.
    mpl_token_metadata::instructions::CreateMetadataAccountV3Cpi {
        __program: metadata_program_info,
        metadata: metadata_info,
        mint: mint_info,
        mint_authority: treasury_info,
        payer: signer_info,
        update_authority: (signer_info, true),
        system_program: system_program_info,
        rent: Some(rent_sysvar_info),
        __args: mpl_token_metadata::instructions::CreateMetadataAccountV3InstructionArgs {
            data: mpl_token_metadata::types::DataV2 {
                name: METADATA_NAME.to_string(),
                symbol: METADATA_SYMBOL.to_string(),
                uri: METADATA_URI.to_string(),
                seller_fee_basis_points: 0,
                creators: None,
                collection: None,
                uses: None,
            },
            is_mutable: true,
            collection_details: None,
        },
    }
    .invoke_signed(&[&[TREASURY, &[TREASURY_BUMP]]])?;

    // Initialize treasury token account.
    create_associated_token_account(
        signer_info,
        treasury_info,
        treasury_tokens_info,
        mint_info,
        system_program_info,
        token_program_info,
        associated_token_program_info,
    )?;

    Ok(())
}
