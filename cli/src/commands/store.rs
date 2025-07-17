use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use crate::cli::{Cli, Commands, StoreCommands};
use crate::log;
use tape_client as tapedrive;
use tape_network::store::{primary as get_primary_store, TapeStore, };
use tape_network::archive::sync_from_block;
use tape_network::snapshot::{create_snapshot, load_from_snapshot};
use chrono::Utc;
use std::env;


pub async fn handle_store_commands(cli: Cli, client: RpcClient) -> Result<()> {
    match cli.command {
        Commands::Store(store) => {
            match store {
                StoreCommands::Stats {} => {
                    let store: TapeStore = get_primary_store()?;
                    let stats = store.get_local_stats()?;
                    log::print_section_header("Local Store Stats");
                    log::print_message(&format!("Number of Tapes: {}", stats.tapes));
                    log::print_message(&format!("Number of Segments: {}", stats.segments));
                    log::print_message(&format!("Size: {} bytes", stats.size_bytes));
                }
                StoreCommands::Resync { tape } => {
                    let tape_pubkey: Pubkey = FromStr::from_str(&tape)?;
                    let (tape_account, _) = tapedrive::get_tape_account(&client, &tape_pubkey).await?;
                    let starting_slot = tape_account.tail_slot;
                    let store: TapeStore = get_primary_store()?;
                    log::print_message(&format!("Re-syncing tape: {}, please wait", tape));
                    sync_from_block(&store, &client, &tape_pubkey, starting_slot).await?;
                    log::print_message("Done");
                }
                StoreCommands::CreateSnapshot { output } => {
                    let snapshot_path = output.unwrap_or_else(|| format!("snapshot_{}.tar.gz", Utc::now().timestamp()));
                    let store: TapeStore = get_primary_store()?;
                    create_snapshot(&store.db, &snapshot_path)?;
                    log::print_message(&format!("Snapshot created at: {}", snapshot_path));
                }
                StoreCommands::LoadSnapshot { input } => {
                    let primary_path = env::current_dir()?.join("db_tapestore");
                    load_from_snapshot(&input, &primary_path)?;
                    log::print_message("Snapshot loaded into primary store");
                }
            }
        }
        _ => {}
    }

    Ok(())
}
