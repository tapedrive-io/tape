mod cli;
mod keypair;
mod log;
mod commands;

use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signer::Signer;

use cli::{Cli, Commands};
use keypair::{load_keypair, get_keypair_path };
use commands::{admin, read, write, misc, network};

#[tokio::main]
async fn main() -> Result<()> {
    log::print_title("⊙⊙ TAPEDRIVE");

    let cli = Cli::parse();
    let rpc_url = cli.cluster.rpc_url();
    let rpc_client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::finalized());
    let keypair_path = get_keypair_path(cli.keypair_path.clone());

    let payer = match load_keypair(&keypair_path) {
        Ok(payer) => payer,
        Err(_) => {
            log::print_message(&format!("Keypair not found at {}.", keypair_path.display()));
            log::print_message("Creating a new keypair...");
            keypair::create_keypair(&keypair_path)?
        }
    };

    // Log the keypair path and payer public key if the command is mutating state
    match cli.command {
        Commands::Initialize { .. } |
        Commands::Epoch { .. } |
        Commands::Write { .. } | 
        Commands::Register { .. } |
        Commands::Mine { .. }
        => {
            log::print_message(&format!(
                "Using keypair: {} from {}",
                payer.pubkey(),
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
            admin::handle_admin_commands(cli, rpc_client, payer).await?;
        }

        // Tape Commands

        Commands::Read { .. } => {
            read::handle_read_command(cli, rpc_client).await?;
        }
        Commands::Write { .. } => {
            write::handle_write_command(cli, rpc_client, payer).await?;
        }

        // Network Commands

        Commands::Register { .. } |
        Commands::Web { .. } |
        Commands::Archive { .. } |
        Commands::Mine { .. } => {
            network::handle_network_commands(cli, rpc_client, payer).await?;
        }

        // Miscellaneous Commands

        _ => {
            misc::handle_misc_commands(cli, rpc_client).await?;
        }
    }

    Ok(())
}

