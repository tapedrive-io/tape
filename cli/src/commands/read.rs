use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Signature;
use std::{
    fs,
    io::{self, Write},
};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::{task, time::Duration};

use crate::cli::{Cli, Commands};
use crate::log;
use tape_client as tapedrive;

pub async fn handle_read_command(cli: Cli, client: RpcClient) -> Result<()> {
    match cli.command {
        Commands::Read { tape, output } => {
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
            let (total_segments, mut current_signature, layout) =
                tapedrive::fetch_tape_metadata(&client, &tape).await?;

            // Read segments
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.white/gray}] {pos}/{len} {wide_msg}")
                    .expect("Failed to set progress style"),
            );
            pb.set_length(total_segments as u64);
            pb.set_position(0);
            pb.set_message("");

            let mut chunks = Vec::new();
            let mut chunk_index = 0;
            let empty_signature = Signature::default();

            loop {
                if current_signature.eq(&empty_signature) {
                    break; // No more segments to read
                }

                let (data, prev_signature) =
                tapedrive::read_tape(
                    &client,
                    &current_signature
                ).await?;

                chunks.push(data.clone());
                chunk_index += (data.len() + 64) / 128;

                pb.set_position(chunk_index as u64);
                current_signature = prev_signature;
            }

            chunks.reverse();

            // Process data
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {wide_msg}")
                    .expect("Failed to set progress style"),
            );
            pb.set_message("Verifying and decompressing data...");

            let result = tapedrive::process_data(chunks, layout)?;

            pb.finish_with_message("");

            log::print_divider();
            if cli.verbose {
                log::print_section_header("Metadata");
                log::print_count(&format!("Original size: {} bytes", result.data.len()));
                log::print_message("Hash verification: Passed");
                log::print_divider();
            }

            // Write output
            match output {
                Some(filename) => {
                    fs::write(&filename, &result.data)?;
                    log::print_message(&format!("Wrote output to: {}", filename));
                }
                None => {
                    io::stdout().write_all(&result.data)?;
                    io::stdout().flush()?;
                }
            }
            log::print_divider();
        }
        _ => {}
    }
    Ok(())
}
