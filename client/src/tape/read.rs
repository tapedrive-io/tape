use anyhow::{Result, anyhow};
use solana_sdk::signature::Signature;
use tape_api::prelude::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use crate::{utils::*, consts::*};

pub async fn read_from_tape(
    client: &RpcClient,
    signature: &Signature,
) -> Result<Vec<u8>> {
    let tx = get_transaction_with_retry(client, signature, MAX_RETRIES).await?;

    let instruction = tx
        .message
        .instructions()
        .into_iter()
        .find(|ix| *ix.program_id(&tx.message.static_account_keys()) == tape_api::ID)
        .ok_or_else(|| anyhow!("No tape instruction found in chunk: {}", signature))?;

    let ix_type = InstructionType::try_from(instruction.data[0])
        .map_err(|_| anyhow!("Invalid instruction type"))?;

    match ix_type {
        InstructionType::Update => {
            // Updates are a bit more complex and not implemented yet. If you need this, use
            // TAPENET to fetch the entire tape in one go.

            todo!("Update instruction not implemented yet");
        }
        InstructionType::Write => {
            Ok(instruction.data[1..].to_vec())
        }
        _ => Err(anyhow!("Unexpected instruction type: {:?}", ix_type)),
    }
}

pub async fn read_linked_chunk(
    client: &RpcClient,
    signature: &Signature,
) -> Result<(Vec<u8>, Signature)> {
    let data = read_from_tape(
        client,
        signature,
    ).await?;

    if data.len() < 64 {
        return Err(anyhow!("Data is too short to contain a valid linked chunk"));
    }

    let prev_chunk : [u8; 64] = data[..64].try_into()?;
    let prev_sig = Signature::from(prev_chunk);
    let chunk = data[64..].to_vec();

    Ok((chunk, prev_sig))
}

