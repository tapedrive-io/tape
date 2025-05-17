use anyhow::{Result, anyhow};
use sha3::{Keccak256, Digest};
use solana_sdk::{
    signature::Signature,
    pubkey::Pubkey,
};
use std::str::FromStr;
use tape_api::prelude::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use crate::{utils::*, consts::*, TapeLayout};

#[derive(Debug)]
pub struct ReadResult {
    pub data: Vec<u8>,
}

/// Fetches tape metadata, validating the tape is finalized, and returns total segments and tail signature.
pub async fn fetch_tape_metadata(
    client: &RpcClient,
    tape_address: &str,
) -> Result<(usize, Signature)> {
    let tape_address = Pubkey::from_str(tape_address)?;
    let (tape, _) = get_tape_account(client, &tape_address).await?;
    if tape.state != u64::from(TapeState::Finalized) {
        return Err(anyhow!("Tape is not finalized, only finalized tapes can be read via the cli"));
    }

    let total_segments = tape.total_segments as usize;
    let tail_signature = Signature::from(tape.tail);

    Ok((total_segments, tail_signature))
}

/// Reads a single segment by transaction signature, returning the segment data and previous signature.
pub async fn read_segment(
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
        InstructionType::Write => {
            let write_ix = ParsedWrite::try_from_bytes(&instruction.data[1..])?;
            let segment_data = write_ix.data.to_vec();
            let prev_signature = Signature::from(write_ix.prev_segment);

            Ok((segment_data, prev_signature))
        }
        _ => Err(anyhow!("Unexpected instruction type: {:?}", ix_type)),
    }
}

/// Processes segments, verifies the hash, and decompresses data.
pub fn process_data(segments: &[(usize, Vec<u8>)]) -> Result<ReadResult> {
    if segments.is_empty() {
        return Err(anyhow!("No data segments found in tape"));
    }

    let mut full_data = Vec::new();
    for (_, segment) in segments {
        full_data.extend_from_slice(segment);
    }

    let version = TapeLayout::try_from(full_data[0])?;
    let hash = &full_data[1..33];
    let raw_data = &full_data[33..];

    let mut hasher = Keccak256::new();
    hasher.update(raw_data);
    if hasher.finalize().as_slice() != hash {
        return Err(anyhow!("Data hash does not match"));
    }

    let data = match version {
        TapeLayout::Compressed => decompress(raw_data)?,
        TapeLayout::Raw => raw_data.to_vec(),
    };

    Ok(ReadResult { data })
}
