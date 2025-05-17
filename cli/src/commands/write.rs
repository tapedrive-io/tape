use anyhow::Result;
use chrono::Utc;
use dialoguer::{theme::ColorfulTheme, Confirm};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use std::{ fs, io::{self, Read}};
use tokio::{task, time::Duration};
use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::{Cli, Commands};
use crate::log;
use tape_client as tapedrive;

const VERIFY_EVERY: usize = 500;
const WAIT_TIME: u64 = 32;

pub async fn handle_write_command(cli: Cli, client: RpcClient, payer: Keypair) -> Result<()> {
    match cli.command {
        Commands::Write {
            filename,
            message,
            remote,
            assume_yes,
            tape_name,
            no_verify,
        } => {
            // Process input data
            let (data, source_desc) = process_input(filename, message, remote).await?;

            // Prepare data (compress, hash, chunk)
            let (chunks, _hash) = tapedrive::prepare_data(&data)?;
            let total_segments = chunks.len();

            // Set tape name, defaulting to timestamp
            let tape_name = tape_name.unwrap_or_else(|| Utc::now().timestamp().to_string());

            // Log write operation details
            if cli.verbose {
                log::print_section_header("Tape Write");
                log::print_message(&format!("Source: {}", source_desc));
                log::print_message(&format!("Tape Name: {}", tape_name));
            }
            log::print_count(&format!("Total Segments: {}", total_segments));
            log::print_divider();

            // Confirm write operation
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

            let pb = ProgressBar::new(total_segments as u64);
            pb.set_style(
                ProgressStyle::default_bar()
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

            pb.set_message("Creating new tape (please wait)...");

            // Phase 1: Create tape
            let (tape_address, writer_address, mut last_signature) =
                tapedrive::create_tape(&client, &payer, &tape_name).await?;

            // Write segments with verification
            let mut last_verified_segments = 0;
            let mut last_verified_tail = last_signature;
            let mut last_tape_segments = 0;
            let mut i = 0;

            pb.set_message("");
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.white/gray}] {pos}/{len} {wide_msg}")
                    .expect("Failed to set progress style"),
            );

            // Phase 2: Write segments
            while i < total_segments {
                let chunk = &chunks[i];
                last_signature = tapedrive::write_segment(
                    &client,
                    &payer,
                    tape_address,
                    writer_address,
                    last_signature,
                    chunk,
                )
                .await?;

                i += 1;
                pb.set_position(i as u64);

                if !no_verify && (i % VERIFY_EVERY == 0 || i == total_segments) {
                    pb.set_message("Verifying...");

                    tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;

                    let (tape_account, _) = tapedrive::get_tape_account(&client, &tape_address).await?;
                    let current_segments = tape_account.total_segments as usize;
                    let segments_added = current_segments - last_tape_segments;

                    let expected_added = if i % VERIFY_EVERY == 0 {
                        VERIFY_EVERY as usize
                    } else {
                        total_segments % VERIFY_EVERY
                    };

                    if segments_added == expected_added {
                        last_verified_tail = last_signature;
                        last_verified_segments = i;
                    } else {
                        log::print_info(&format!("Verification failed at segment {}", i));
                        i = last_verified_segments;
                        last_signature = last_verified_tail;

                        pb.set_position(i as u64);
                        log::print_message(&format!("Retrying from segment {}", i));
                    }

                    last_tape_segments = current_segments;
                }
            }

            // Phase 3: Finalize tape
            tapedrive::finalize_tape(&client, &payer, tape_address, writer_address, last_signature)
                .await?;

            pb.finish_with_message("");

            log::print_divider();

            // Log write results
            if cli.verbose {
                log::print_divider();
                log::print_section_header("Metadata");
                log::print_count(&format!("Tape Address: {}", tape_address));
                log::print_count(&format!("Total Segments: {}", total_segments));
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
