use anyhow::Result;
use std::str::FromStr;
use dialoguer::{theme::ColorfulTheme, Confirm};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{signature::Keypair, signer::Signer, pubkey::Pubkey};

use tape_api::prelude::*;
use tape_client::{register::register_miner, get_miner_account};
use tape_network::{
    archive::archive_loop,
    mine::mine_loop,
    web::web_loop,
};

const DEVNET: &str = "https://devnet.tapedrive.io/api";

use crate::cli::{Cli, Commands};
use crate::log;

pub async fn handle_network_commands(cli: Cli, client: RpcClient, payer: Keypair) -> Result<()> {

    log::print_divider();

    match cli.command {

        Commands::Web { port } => {
            let port = port.unwrap_or(3000);

            log::print_info("Starting web RPC service...");
            log::print_message(format!("Listening on port {}", port).as_str());

            let secondary_store = tape_network::store::secondary()?;
            web_loop(secondary_store, port).await?;
        }

        Commands::Archive { starting_slot, trusted_peer } => {

            // Use the public devnet peer if none is provided
            let trusted_peer = match client.url() {
                url if url.contains("devnet") => {
                    Some(trusted_peer.unwrap_or(DEVNET.to_string()))
                }
                _ => trusted_peer
            };

            log::print_info("Starting archive service...");

            let primary_store = tape_network::store::primary()?;
            archive_loop(&primary_store, &client, starting_slot, trusted_peer).await?;
        }

        Commands::Mine { pubkey, name } => {
            log::print_info("Starting mining service...");

            // Either the user provided a pubkey or a name, but not both. A name is optional. If
            // the user provided a pubkey, we use that. If the user provided a name, we derive the
            // pubkey from the name. If neither is provided, we use the default name ("default").

            let miner_address = match (pubkey, name.clone()) {
                (Some(pubkey), _) => {
                    let miner_address = Pubkey::from_str(&pubkey)?;
                    miner_address
                }
                (_, Some(name)) => {
                    let (miner_address, _) = miner_pda(payer.pubkey(), to_name(&name));
                    miner_address
                }
                _ => {
                    let (miner_address, _) = miner_pda(payer.pubkey(), to_name("default"));
                    miner_address
                }
            };

            // Check if the miner address is valid and registered, if not, then register it using
            // the provided name or the default name ("default").

            log::print_message(&format!("Using miner address: {}", miner_address));

            let miner_account = get_miner_account(&client, &miner_address).await;
            if miner_account.is_err() {
                log::print_message("Miner not registered, registering now...");
                let name = name.unwrap_or_else(|| "default".to_string());
                register_miner(&client, &payer, &name).await?;
                log::print_message("Miner registered successfully");
                log::print_message(&format!("Name: {}", name));
            }

            let secondary_store = tape_network::store::secondary()?;
            mine_loop(&secondary_store, &client, &miner_address, &payer).await?;
        }

        Commands::Register { name } => {
            log::print_info("Registering miner...");

            let (miner_address, _) = miner_pda(payer.pubkey(), to_name(&name));

            let proceed = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("→ Are you sure?")
                .default(false)
                .interact()
                .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?;
            if !proceed {
                log::print_error("Write operation cancelled");
                return Ok(());
            }

            register_miner(&client, &payer, &name).await?;

            log::print_section_header("Miner Registered");
            log::print_message(&format!("Name: {}", name));
            log::print_message(&format!("Address: {}", miner_address));

            log::print_divider();
            log::print_info("More info:");
            log::print_title(&format!("tapedrive get-miner {}", miner_address));
            log::print_divider();
        }

        _ => {}
    }
    Ok(())
}

