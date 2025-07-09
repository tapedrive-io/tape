mod cli;
mod keypair;
mod log;
mod commands;

use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;

use cli::{Cli, Commands};
use keypair::{ get_payer, get_keypair_path };
use commands::{admin, read, write, misc, network, claim};

#[tokio::main]
async fn main() -> Result<()> {
    log::print_title("⊙⊙ TAPEDRIVE");

    let cli = Cli::parse();
    let rpc_url = cli.cluster.rpc_url();
    let rpc_client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::finalized());
    let keypair_path = get_keypair_path(cli.keypair_path.clone());

    match cli.command {
        Commands::Initialize { .. } |
        Commands::Epoch { .. } |
        Commands::Write { .. } | 
        Commands::Register { .. } |
        Commands::Mine { .. }
        => {
            log::print_message(&format!(
                "Using keypair from {}",
                keypair_path.display()
            ));
        }
        _ => {}
    }

    log::print_message(&format!("Connected to: {}", rpc_url));

    match cli.command {
        // Admin Commands

        Commands::Initialize { .. } | 
        Commands::Epoch { .. } => {
            let payer = get_payer(keypair_path)?;
            admin::handle_admin_commands(cli, rpc_client, payer).await?;
        }

        // Tape Commands

        Commands::Read { .. } => {
            read::handle_read_command(cli, rpc_client).await?;
        }
        Commands::Write { .. } => {
            let payer = get_payer(keypair_path)?;
            write::handle_write_command(cli, rpc_client, payer).await?;
        }

        // Miner Commands

        Commands::Claim { .. } => {
            let payer = get_payer(keypair_path)?;
            claim::handle_claim_command(cli, rpc_client, payer).await?;
        }

        // Network Commands

        Commands::Register { .. } |
        Commands::Web { .. } |
        Commands::Archive { .. } |
        Commands::Mine { .. } => {
            let payer = get_payer(keypair_path)?;
            network::handle_network_commands(cli, rpc_client, payer).await?;
        }

        // Miscellaneous Commands

        _ => {
            misc::handle_misc_commands(cli, rpc_client).await?;
        }
    }

    Ok(())
}
