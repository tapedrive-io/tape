use tape_api::prelude::*;
use steel::*;

pub fn process_write(accounts: &[AccountInfo<'_>], data: &[u8]) -> ProgramResult {
    let args = ParsedWrite::try_from_bytes(data)?;
    let [
        signer_info, 
        tape_info,
        writer_info, 
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    signer_info.is_signer()?;

    let tape = tape_info
        .as_account_mut::<Tape>(&tape_api::ID)?
        .assert_mut_err(
            |p| p.authority == *signer_info.key,
            ProgramError::MissingRequiredSignature,
        )?;

    let writer = writer_info
        .as_account_mut::<Writer>(&tape_api::ID)?
        .assert_mut_err(
            |p| p.tape == *tape_info.key,
            ProgramError::InvalidAccountData,
        )?;

    let (tape_address, _tape_bump) = tape_pda(*signer_info.key, &tape.name);
    let (writer_address, _writer_bump) = writer_pda(tape_address);

    tape_info.has_address(&tape_address)?;
    writer_info.has_address(&writer_address)?;
        
    check_condition(
        tape.state.eq(&u64::from(TapeState::Created)) ||
        tape.state.eq(&u64::from(TapeState::Writing)),
        TapeError::UnexpectedState,
    )?;

    // Convert the data to a canonical segment of data and write it to the Merkle tree (all
    // segments are written as SEGMENT_SIZE bytes, no matter the size of the data)
    let segment = Segment::try_from_bytes(&args.data)?;
    let segment_number = tape.total_segments;

    write_chunks(
        &mut writer.state,
        segment_number,
        &segment,
    )?;

    tape.total_segments   += 1;
    tape.total_size       += args.data.len() as u64;
    tape.merkle_root       = writer.state.get_root().to_bytes();
    tape.tail              = args.prev_segment;
    tape.state             = TapeState::Writing.into();

    WriteEvent {
        segment: segment_number,
        address: tape_address.to_bytes(),
    }
    .log();

    Ok(())
}
