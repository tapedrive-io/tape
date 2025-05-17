use anyhow::{anyhow, Result};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use solana_client::nonblocking::rpc_client::RpcClient;

use tape_api::prelude::*;
use crate::utils::*;

pub async fn advance(client: &RpcClient, signer: &Keypair) -> Result<Signature> {
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(250_000);
    let advance_ix = build_advance_ix(signer.pubkey());

    let blockhash_bytes = get_latest_blockhash(client).await?;
    let recent_blockhash = deserialize(&blockhash_bytes)?;
    let tx = Transaction::new_signed_with_payer(
        &[
            compute_budget_ix, 
            advance_ix
        ],
        Some(&signer.pubkey()),
        &[signer],
        recent_blockhash,
    );

    send_and_confirm_transaction(client, &tx)
        .await
        .map_err(|e| anyhow!("Failed to advance program: {}", e))
        .map(|_| Signature::default())
}
