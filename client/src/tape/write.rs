use anyhow::Result;
use solana_sdk::{
    signature::{Keypair, Signer, Signature},
    pubkey::Pubkey,
};
use tape_api::prelude::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use crate::{
    consts::*,
    utils::*,
};

/// Writes a chunk of data to an unlinked tape, returning the signature and the estimated 
/// segment count.
pub async fn write_to_tape(
    client: &RpcClient,
    signer: &Keypair,
    tape_address: Pubkey,
    writer_address: Pubkey,
    data: &[u8],
) -> Result<(Signature, usize)> {

    let segment_count = (data.len() + SEGMENT_SIZE - 1) / SEGMENT_SIZE;

    let instruction = build_write_ix(
        signer.pubkey(),
        tape_address,
        writer_address,
        data,
    );

    let signature = send_with_retry(client, &instruction, signer, MAX_RETRIES).await?;

    Ok((signature, segment_count))
}

/// Writes a chunk of data to a linked tape and returns the signature and the estimated
/// segment count.
pub async fn write_linked_chunk(
    client: &RpcClient,
    signer: &Keypair,
    tape_address: Pubkey,
    writer_address: Pubkey,
    prev_signature: Signature,
    data: &[u8],
) -> Result<(Signature, usize)> {

    let prev_segment: [u8; 64] = prev_signature
        .as_ref()
        .try_into()
        .unwrap();

    let payload = &[
        prev_segment.as_ref(), 
        data.as_ref(),
    ].concat();

    write_to_tape(
        client,
        signer,
        tape_address,
        writer_address,
        payload,
    ).await
}
