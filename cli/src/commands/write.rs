use anyhow::Result;
use chrono::Utc;
use dialoguer::{theme::ColorfulTheme, Confirm};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use std::{ fs, io::{self, Read}};
use tokio::{task, time::Duration};
use indicatif::{ProgressBar, ProgressStyle};

use tape_api::prelude::*;
use crate::cli::{Cli, Commands};
use crate::log;
use tape_client as tapedrive;

const VERIFY_EVERY: usize       = 500;
const WAIT_TIME: u64            = 32;
const SEGMENTS_PER_TX: usize    = 7; // 7 x 128 = 896 bytes
const SAFE_SIZE : usize         = SEGMENT_SIZE * SEGMENTS_PER_TX;

pub async fn handle_write_command(cli: Cli, client: RpcClient, payer: Keypair) -> Result<()> {
    match cli.command {
        Commands::Write {
            filename,
            message,
            remote,
            assume_yes,
            tape_name,
            no_verify,
            raw,
        } => {
            let should_verify = !no_verify;

            // Process input data
            let (data, source_desc) = process_input(filename, message, remote).await?;
            let layout = match raw {
                true => TapeLayout::Raw,
                false => TapeLayout::Compressed,
            };

            // Apply compression if requested and chunk
            let with_layout = tapedrive::prepare_data(&data, layout)?;
            let chunks : Vec<_> = with_layout
                .chunks(SAFE_SIZE)
                .map(|c| c.to_vec())
                .collect();

            let tape_name = tape_name
                .unwrap_or_else(|| Utc::now().timestamp().to_string());

            if cli.verbose {
                log::print_section_header("Tape Write");
                log::print_message(&format!("Source: {}", source_desc));
                log::print_message(&format!("Tape Name: {}", tape_name));
            }
            log::print_count(&format!("Total Chunks: {}", chunks.len()));
            log::print_divider();

            if !assume_yes {
                let proceed = Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt("→ Begin writing to tape?")
                    .default(false)
                    .interact()
                    .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?;
                if !proceed {
                    log::print_error("Write operation cancelled");
                    return Ok(());
                }
            }
            log::print_divider();

            // setup progress bar
            let pb = ProgressBar::new(chunks.len() as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} {wide_msg}")
                    .expect("Failed to set progress style"),
            );
            let pb_clone = pb.clone();
            task::spawn(async move {
                while !pb_clone.is_finished() {
                    pb_clone.tick();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });

            // Create the tape
            pb.set_message("Creating new tape (please wait)...");
            let (tape_address, writer_address, mut last_sig) =
                tapedrive::create_tape(&client, &payer, &tape_name, layout).await?;

            // Write the tape
            pb.set_message("");
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.white/gray}] {pos}/{len} {wide_msg}")
                    .expect("Failed to set progress style"),
            );

            let mut expected_segments = 0usize;
            let mut last_good_chunk = 0usize;
            let mut last_good_segments = 0usize;
            let mut last_good_sig = last_sig;
            let mut i = 0usize;

            while i < chunks.len() {
                let chunk = &chunks[i];
                let (new_sig, used) = tapedrive::write_tape(
                    &client, 
                    &payer, 
                    tape_address, 
                    writer_address, 
                    last_sig, 
                    chunk
                ).await?;

                last_sig = new_sig;
                expected_segments += used as usize;

                i += 1;
                pb.set_position(i as u64);

                let is_checkpoint = i % VERIFY_EVERY == 0 || i == chunks.len();
                if should_verify && is_checkpoint {
                    pb.set_message("Verifying...");
                    tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;

                    let (acct, _) = tapedrive::get_tape_account(&client, &tape_address).await?;
                    let onchain = acct.total_segments as usize;

                    if onchain == expected_segments {
                        last_good_chunk = i;
                        last_good_segments = expected_segments;
                        last_good_sig = last_sig;
                    } else {
                        log::print_info(&format!(
                            "Verification failed at chunk {}; onchain {}, expected {}",
                            i, onchain, expected_segments
                        ));
                        i = last_good_chunk;
                        expected_segments = last_good_segments;
                        last_sig = last_good_sig;
                        pb.set_position(i as u64);
                        log::print_message(&format!("Retrying from chunk {}", i));
                    }

                    pb.set_message("");
                }
            }

            // Finalize the tape (prevents further writes and reclaims sol)
            tapedrive::finalize_tape(
                &client,
                &payer,
                tape_address,
                writer_address,
                last_sig,
            ).await?;

            pb.finish_with_message("");
            log::print_divider();

            if cli.verbose {
                log::print_divider();
                log::print_section_header("Metadata");
                log::print_count(&format!("Tape Address: {}", tape_address));
                log::print_count(&format!("Total Chunks: {}", chunks.len()));
            }

            log::print_divider();
            log::print_info("To read the tape, run:");
            log::print_title(&format!("tapedrive read {}", tape_address));
            log::print_divider();
        }
        _ => {}
    }
    Ok(())
}

/// Processes the input data source (file, message, or remote URL) and returns the data and its description.
pub async fn process_input(
    filename: Option<String>,
    message: Option<String>,
    remote: Option<String>,
) -> Result<(Vec<u8>, String)> {
    match (filename, message, remote) {
        (Some(f), None, None) => Ok((fs::read(&f)?, f)),
        (None, Some(m), None) => {
            if m == "-" {
                let stdin_data = read_from_stdin()?;
                if stdin_data.is_empty() {
                    return Err(anyhow::anyhow!("No data provided via piped input"));
                }
                Ok((stdin_data, "piped input".to_string()))
            } else {
                Ok((m.as_bytes().to_vec(), "command-line message".to_string()))
            }
        }
        (None, None, Some(url)) => {
            let response = reqwest::get(&url).await?;
            if !response.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Failed to fetch remote file: HTTP {}",
                    response.status()
                ));
            }
            let data = response.bytes().await?.to_vec();
            Ok((data, url))
        }
        _ => Err(anyhow::anyhow!(
            "Must provide exactly one of: filename, message, or remote URL"
        )),
    }
}

/// Reads data from stdin into a vector of bytes.
fn read_from_stdin() -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    io::stdin().read_to_end(&mut buffer)?;
    Ok(buffer)
}
