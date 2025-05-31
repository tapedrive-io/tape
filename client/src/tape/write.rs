use anyhow::Result;
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

/// Creates a new tape and returns the tape address, writer address, and initial signature.
pub async fn create_tape(
    client: &RpcClient,
    signer: &Keypair,
    name: &str,
    layout: TapeLayout,
) -> Result<(Pubkey, Pubkey, Signature)> {
    let (tape_address, _tape_bump) = tape_pda(signer.pubkey(), &to_name(name));
    let (writer_address, _writer_bump) = writer_pda(tape_address);

    let create_ix = build_create_ix(signer.pubkey(), name, layout);
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
pub async fn write_tape(
    client: &RpcClient,
    signer: &Keypair,
    tape_address: Pubkey,
    writer_address: Pubkey,
    prev_signature: Signature,
    data: &[u8],
) -> Result<(Signature, usize)> {
    let prev_segment: [u8; 64] = prev_signature.as_ref().try_into().unwrap();
    let payload = &[
        prev_segment.as_ref(), 
        data.as_ref(),
    ].concat();

    let segment_count = (payload.len() + SEGMENT_SIZE - 1) / SEGMENT_SIZE;

    let instruction = build_write_ix(
        signer.pubkey(),
        tape_address,
        writer_address,
        payload,
    );

    let signature = send_with_retry(client, &instruction, signer, MAX_RETRIES).await?;

    Ok((signature, segment_count))
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

/// Prepares data for writing (compresses if needed), returning the segments.
pub fn prepare_data(data: &[u8], layout: TapeLayout) -> Result<Vec<u8>> {
    let processed = match layout {
        TapeLayout::Compressed => compress(data)?,
        _ => data.to_vec(),
    };

    Ok(processed)
}
