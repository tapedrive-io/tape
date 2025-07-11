use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{signature::Signature, pubkey::Pubkey};
use std::{
    fs,
    io::{self, Write},
};
use std::str::FromStr;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::{task, time::Duration};

use crate::cli::{Cli, Commands};
use crate::log;
use tape_client::{
    decode_tape, get_tape_account, read_linked_chunk, TapeHeader
};

pub async fn handle_read_command(cli: Cli, client: RpcClient) -> Result<()> {
    match cli.command {
        Commands::Read { tape, output } => {
            let tape_address = Pubkey::from_str(&tape)
                .map_err(|_| anyhow::anyhow!("Invalid tape address: {}", tape))?;

            log::print_message("Reading tape...");
            log::print_divider();

            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {wide_msg}")
                    .expect("Failed to set progress style"),
            );

            // Spawn a task to steadily tick the spinner
            let pb_clone = pb.clone();
            let _tick_handle = task::spawn(async move {
                while !pb_clone.is_finished() {
                    pb_clone.tick();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });

            // Fetch tape metadata
            pb.set_message("Fetching tape metadata...");
            let (tape, _) = get_tape_account(&client, &tape_address).await?;
            let header = &TapeHeader::try_from_bytes(&tape.header)?;

            // Read segments
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.white/gray}] {pos}/{len} {wide_msg}")
                    .expect("Failed to set progress style"),
            );
            pb.set_length(tape.total_segments);
            pb.set_position(0);
            pb.set_message("");

            let empty_signature = Signature::default();
            let mut current_signature = Signature::from(header.tail_signature);
            let mut chunks = Vec::new();
            let mut chunk_index = 0;

            loop {
                if current_signature.eq(&empty_signature) {
                    break; // No more segments to read
                }

                let (data, prev_signature) =
                    read_linked_chunk(&client, &current_signature).await?;

                chunks.push(data.clone());
                chunk_index += (data.len() + 64) / 128;

                pb.set_position(chunk_index as u64);
                current_signature = prev_signature;
            }

            chunks.reverse();
            let data: Vec<u8> = chunks.into_iter().flatten().collect();

            // Process data
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {wide_msg}")
                    .expect("Failed to set progress style"),
            );
            pb.set_message("Verifying and decompressing data...");

            let result = decode_tape(data, &header)?;

            pb.finish_with_message("");
            log::print_divider();
            if cli.verbose {
                log::print_section_header("Metadata");
                log::print_count(&format!("Size: {} bytes", result.len()));
                log::print_divider();
            }

            // Write output
            match output {
                Some(filename) => {
                    fs::write(&filename, result)?;
                    log::print_message(&format!("Wrote output to: {}", filename));
                }
                None => {
                    io::stdout().write_all(&result)?;
                    io::stdout().flush()?;
                }
            }
            log::print_divider();
        }
        _ => {}
    }
    Ok(())
}
