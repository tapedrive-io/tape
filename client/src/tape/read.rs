use anyhow::{Result, anyhow};
use solana_sdk::{
    signature::Signature,
    pubkey::Pubkey,
};
use std::str::FromStr;
use tape_api::prelude::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use crate::{utils::*, consts::*};

#[derive(Debug)]
pub struct ReadResult {
    pub data: Vec<u8>,
}

/// Fetches tape metadata, validating the tape is finalized, and returns total segments and tail signature.
pub async fn fetch_tape_metadata(
    client: &RpcClient,
    tape_address: &str,
) -> Result<(usize, Signature, TapeLayout)> {
    let tape_address = Pubkey::from_str(tape_address)?;
    let (tape, _) = get_tape_account(client, &tape_address).await?;
    if tape.state != u32::from(TapeState::Finalized) {
        return Err(anyhow!("Tape is not finalized, only finalized tapes can be read via the cli"));
    }

    let total_segments = tape.total_segments as usize;
    let tail_signature = Signature::from(tape.opaque_data);
    let tape_layout = TapeLayout::try_from(tape.layout)
        .map_err(|_| anyhow!("Invalid tape layout"))?;

    Ok((total_segments, tail_signature, tape_layout))
}

/// Reads a single segment by transaction signature, returning the segment data and previous signature.
pub async fn read_tape(
    client: &RpcClient,
    signature: &Signature,
) -> Result<(Vec<u8>, Signature)> {
    let tx = get_transaction_with_retry(client, signature, MAX_RETRIES).await?;

    let instruction = tx
        .message
        .instructions()
        .into_iter()
        .find(|ix| *ix.program_id(&tx.message.static_account_keys()) == tape_api::ID)
        .ok_or_else(|| anyhow!("No tape instruction found in segment: {}", signature))?;

    let ix_type = InstructionType::try_from(instruction.data[0])
        .map_err(|_| anyhow!("Invalid instruction type"))?;

    match ix_type {
        InstructionType::Create => {
            Ok((Vec::new(), Signature::default()))
        },
        InstructionType::Write => {
            let write_ix = ParsedWrite::try_from_bytes(&instruction.data[1..])?;
            let segment_data = write_ix.data.to_vec();
            let opaque_data : [u8; 64] = segment_data[..64].try_into()?;
            let segment_data = segment_data[64..].to_vec();
            let prev_signature = Signature::from(opaque_data);

            Ok((segment_data, prev_signature))
        }
        _ => Err(anyhow!("Unexpected instruction type: {:?}", ix_type)),
    }
}

/// Processes segments, verifies the hash, and decompresses data.
pub fn process_data(chunks: Vec<Vec<u8>>, layout:TapeLayout) -> Result<ReadResult> {
    if chunks.is_empty() {
        return Err(anyhow!("No data chunks found in tape"));
    }

    let mut full_data = Vec::new();
    for chunk in chunks {
        full_data.extend_from_slice(&chunk);
    }

    let data = match layout {
        TapeLayout::Compressed => decompress(&full_data)?,
        _ => full_data.to_vec(),
    };

    Ok(ReadResult { data })
}
