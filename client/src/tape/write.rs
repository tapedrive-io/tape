use anyhow::Result;
use sha3::{Keccak256, Digest};
use solana_sdk::{
    signature::{Keypair, Signer, Signature},
    transaction::Transaction,
    pubkey::Pubkey,
};
use tape_api::prelude::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use crate::{
    consts::*,
    utils::*,
};
use super::TapeLayout;

/// Creates a new tape and returns the tape address, writer address, and initial signature.
pub async fn create_tape(
    client: &RpcClient,
    signer: &Keypair,
    name: &str,
) -> Result<(Pubkey, Pubkey, Signature)> {
    let (tape_address, _tape_bump) = tape_pda(signer.pubkey(), &to_name(name));
    let (writer_address, _writer_bump) = writer_pda(tape_address);

    let create_ix = build_create_ix(signer.pubkey(), name);
    let blockhash_bytes = get_latest_blockhash(client).await?;
    let recent_blockhash = deserialize(&blockhash_bytes)?;
    let create_tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&signer.pubkey()),
        &[signer],
        recent_blockhash,
    );

    let signature = send_and_confirm(client, &create_tx).await?;

    Ok((tape_address, writer_address, signature))
}

/// Writes a single segment to the tape, returning the new signature.
pub async fn write_segment(
    client: &RpcClient,
    signer: &Keypair,
    tape_address: Pubkey,
    writer_address: Pubkey,
    prev_signature: Signature,
    chunk: &[u8],
) -> Result<Signature> {
    let prev_segment: [u8; 64] = prev_signature.as_ref().try_into().unwrap();
    let instruction = build_write_ix(
        signer.pubkey(),
        tape_address,
        writer_address,
        Some(prev_segment),
        chunk,
    );

    let signature = send_with_retry(client, &instruction, signer, MAX_RETRIES).await?;

    Ok(signature)
}

/// Finalizes the tape with the last segment's signature.
pub async fn finalize_tape(
    client: &RpcClient,
    signer: &Keypair,
    tape_address: Pubkey,
    writer_address: Pubkey,
    last_signature: Signature,
) -> Result<()> {
    let tail_segment: [u8; 64] = last_signature.as_ref().try_into().unwrap();
    let finalize_ix = build_finalize_ix(
        signer.pubkey(),
        tape_address,
        writer_address,
        tail_segment,
    );

    let blockhash_bytes = get_latest_blockhash(client).await?;
    let recent_blockhash = deserialize(&blockhash_bytes)?;
    let finalize_tx = Transaction::new_signed_with_payer(
        &[finalize_ix],
        Some(&signer.pubkey()),
        &[signer],
        recent_blockhash,
    );

    send(client, &finalize_tx).await?;

    Ok(())
}

/// Prepares data for writing (compresses and hashes it), returning the chunks and hash.
pub fn prepare_data(data: &[u8], use_compression: bool) -> Result<(Vec<Vec<u8>>, [u8; 32])> {
    let data = if use_compression {
        compress(data)?
    } else {
        data.to_vec()
    };

    let mut hasher = Keccak256::new();
    hasher.update(&data);
    let hash = hasher.finalize();

    let mut full_data = Vec::new();

    if use_compression {
        full_data.push(TapeLayout::Compressed.into());
    } else {
        full_data.push(TapeLayout::Raw.into());
    }

    full_data.extend_from_slice(&hash);
    full_data.extend_from_slice(&data);

    let safe_size = SEGMENT_SIZE.min(128*7);

    let chunks: Vec<Vec<u8>> = full_data.chunks(safe_size).map(|c| c.to_vec()).collect();
    Ok((chunks, hash.into()))
}
