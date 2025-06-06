use anyhow::{Result, bail};
use dialoguer::{theme::ColorfulTheme, Confirm};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::{Keypair, Signature};
use chrono::Utc;
use std::io::Read;
use tokio::{task, time::Duration};
use indicatif::{ProgressBar, ProgressStyle};

use mime::Mime;
use mime_guess::MimeGuess;

use tape_api::prelude::*;
use tape_client::{
    MimeType, 
    CompressionAlgo,
    EncryptionAlgo,
    TapeFlags,
    TapeHeader,
    encode_tape,
    create_tape,
    write_linked_chunk,
    finalize_tape,
    get_tape_account,
};

use crate::cli::{Cli, Commands};
use crate::log;

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
            tape_name,
        } => {

            let (data, source, mime) = process_input(filename, message, remote).await?;
            let (mime_type, mime_str) = mime_to_type(&mime);

            let compression_algo = CompressionAlgo::Gzip;
            let encryption_algo  = EncryptionAlgo::None; // No encryption for now
            let flags = TapeFlags::Linked;

            let mut header = TapeHeader::new(
                mime_type, 
                compression_algo,
                encryption_algo,
                flags 
            );

            header.mime_str = mime_str;

            let encoded = encode_tape(&data, &header)?;
            let chunks : Vec<_> = encoded
                .chunks(SAFE_SIZE)
                .map(|c| c.to_vec())
                .collect();

            let tape_name = tape_name
                .unwrap_or_else(|| Utc::now().timestamp().to_string());

            if cli.verbose {
                log::print_section_header("Tape Write");
                log::print_message(&format!("Source: {}", source));
                log::print_message(&format!("Tape Name: {}", tape_name));
                log::print_message(&format!("MIME Type: {}", mime));
                log::print_message(&format!("Compression: {:?}", compression_algo));
                log::print_message(&format!("Encryption: {:?}", encryption_algo));
                log::print_message(&format!("Flags: {:?}", flags));
            }
            log::print_count(&format!("Total Chunks: {}", chunks.len()));
            log::print_divider();

            // Ask for confirmation before proceeding
            let proceed = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("→ Begin writing to tape?")
                .default(false)
                .interact()
                .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?;
            if !proceed {
                log::print_error("Write operation cancelled");
                return Ok(());
            }
            log::print_divider();

            // Create a progress bar
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
            let (tape_address, writer_address, _sig) =
                create_tape(&client, &payer, &tape_name, header).await?;

            // Write the tape
            pb.set_message("");
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.white/gray}] {pos}/{len} {wide_msg}")
                    .expect("Failed to set progress style"),
            );

            
            let mut i = 0;
            let mut last_sig = Signature::default();
            let mut expected_segments = 0;
            let mut last_good_chunk = 0;
            let mut last_good_segments = 0;
            let mut last_good_sig = last_sig;

            while i < chunks.len() {
                let chunk = &chunks[i];
                let (new_sig, used) = write_linked_chunk(
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

                let is_checkpoint = i % VERIFY_EVERY == 0;
                let is_last_write = i == chunks.len();

                if is_checkpoint || is_last_write {
                    pb.set_message("Verifying...");
                    tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;

                    let (acct, _) = get_tape_account(&client, &tape_address).await?;
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

            header.tail_signature = last_sig.into();

            // Finalize the tape (prevents further writes and reclaims sol)
            finalize_tape(
                &client,
                &payer,
                tape_address,
                writer_address,
                header,
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

            //let mut expected_segments = 0usize;
            //let mut last_good_chunk = 0usize;
            //let mut last_good_segments = 0usize;
            //let mut last_good_sig = last_sig;
            //let mut i = 0usize;
            //
            //while i < chunks.len() {
            //    let chunk = &chunks[i];
            //    let (new_sig, used) = tapedrive::write_tape(
            //        &client, 
            //        &payer, 
            //        tape_address, 
            //        writer_address, 
            //        last_sig, 
            //        chunk
            //    ).await?;
            //
            //    last_sig = new_sig;
            //    expected_segments += used as usize;
            //
            //    i += 1;
            //    pb.set_position(i as u64);
            //
            //    let is_checkpoint = i % VERIFY_EVERY == 0 || i == chunks.len();
            //    if should_verify && is_checkpoint {
            //        pb.set_message("Verifying...");
            //        tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;
            //
            //        let (acct, _) = tapedrive::get_tape_account(&client, &tape_address).await?;
            //        let onchain = acct.total_segments as usize;
            //
            //        if onchain == expected_segments {
            //            last_good_chunk = i;
            //            last_good_segments = expected_segments;
            //            last_good_sig = last_sig;
            //        } else {
            //            log::print_info(&format!(
            //                "Verification failed at chunk {}; onchain {}, expected {}",
            //                i, onchain, expected_segments
            //            ));
            //            i = last_good_chunk;
            //            expected_segments = last_good_segments;
            //            last_sig = last_good_sig;
            //            pb.set_position(i as u64);
            //            log::print_message(&format!("Retrying from chunk {}", i));
            //        }
            //
            //        pb.set_message("");
            //    }
            //}
            //
            //// Finalize the tape (prevents further writes and reclaims sol)
            //tapedrive::finalize_tape(
            //    &client,
            //    &payer,
            //    tape_address,
            //    writer_address,
            //    last_sig,
            //).await?;
            //
            //pb.finish_with_message("");
            //log::print_divider();
            //
            //if cli.verbose {
            //    log::print_divider();
            //    log::print_section_header("Metadata");
            //    log::print_count(&format!("Tape Address: {}", tape_address));
            //    log::print_count(&format!("Total Chunks: {}", chunks.len()));
            //}
            //
            //log::print_divider();
            //log::print_info("To read the tape, run:");
            //log::print_title(&format!("tapedrive read {}", tape_address));
            //log::print_divider();
        }
        _ => {}
    }
    Ok(())
}

/// Helper function to process input based on the provided parameters. 
/// Returns the data, source description, and MIME type.
pub async fn process_input(
    filename: Option<String>,
    message: Option<String>,
    remote: Option<String>,
) -> Result<(Vec<u8>, String, Mime)> {

    match (filename, message, remote) {
        // File on disk
        (Some(path_str), None, None) => {
            let data = std::fs::read(&path_str)?;
            let source = path_str.clone();

            // Use mime_guess on the file extension
            let mime = MimeGuess::from_path(&std::path::Path::new(&path_str))
                .first_or_octet_stream();

            Ok((data, source, mime))
        }

        // Inline message or piped stdin
        (None, Some(m), None) => {
            if m == "-" {
                // read all of stdin
                let stdin_data = read_from_stdin()?;
                if stdin_data.is_empty() {
                    bail!("No data provided via piped input");
                }
                let source = "piped input".to_string();
                // Treat piped stdin as binary/octet 
                let mime = default_octet();
                return Ok((stdin_data, source, mime));
            } else {
                // plain command‐line string
                let data = m.as_bytes().to_vec();
                let source = "command-line message".to_string();
                let mime: Mime = "text/plain".parse().unwrap();
                return Ok((data, source, mime));
            }
        }

        // Remote URL
        (None, None, Some(url)) => {
            // Fetch the URL
            let response = reqwest::get(&url).await?;
            if !response.status().is_success() {
                bail!(
                    "Failed to fetch remote file: HTTP {}",
                    response.status()
                );
            }

            // Try to get Content-Type header first
            let mime = if let Some(ct_header) = response.headers().get(reqwest::header::CONTENT_TYPE) {
                match ct_header.to_str() {
                    Ok(s) => s.parse().unwrap_or_else(|_| default_octet()),
                    Err(_) => default_octet(),
                }
            } else {
                // No Content‐Type header; fall back to guessing based on URL extension
                MimeGuess::from_path(&std::path::Path::new(&url))
                    .first_or_octet_stream()
            };

            let data = response.bytes().await?.to_vec();
            let source = url.clone();
            Ok((data, source, mime))
        }

        // Anything else (zero or more than one provided)
        _ => bail!(
            "Must provide exactly one of: <FILE>, -m <MSG>, or -r <URL>"
        ),
    }
}

/// Reads data from stdin into a vector of bytes.
fn read_from_stdin() -> std::io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    std::io::stdin().read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// Helper to default to octet-stream if we can’t guess anything.
fn default_octet() -> Mime {
    // application/octet-stream
    "application/octet-stream".parse().unwrap()
}

/// Given a `Mime`, return:
/// - the `u8` code for a matching `MimeType` variant, or `MimeType::Custom as u8` if none match,
/// - plus a `[u8; NAME_LEN]` buffer containing either all zeros (for known types),
///   or a null‐padded ASCII‐lowercase copy of the full MIME string (for Custom).
fn mime_to_type(mime: &Mime) -> (MimeType, [u8; NAME_LEN]) {

    let empty = [0u8; NAME_LEN];
    let (t, s) = (mime.type_().as_str(), mime.subtype().as_str());
    let t = t.to_ascii_lowercase();
    let s = s.to_ascii_lowercase();

    let code: MimeType = match (t.as_str(), s.as_str()) {
        // Unknown or octet-stream -> keep Unknown (0)
        ("application", "octet-stream") => MimeType::Unknown,

        // Image formats
        ("image", "png") => MimeType::ImagePng,
        ("image", "jpeg") | ("image", "jpg") => MimeType::ImageJpeg,
        ("image", "gif") => MimeType::ImageGif,
        ("image", "webp") => MimeType::ImageWebp,
        ("image", "bmp") => MimeType::ImageBmp,
        ("image", "tiff") | ("image", "tif") => MimeType::ImageTiff,

        // Document formats
        ("application", "pdf") => MimeType::ApplicationPdf,
        ("application", "msword") => MimeType::ApplicationMsword,
        ("application", "vnd.openxmlformats-officedocument.wordprocessingml.document") => MimeType::ApplicationDocx,
        ("application", "vnd.oasis.opendocument.text") => MimeType::ApplicationOdt,

        // Text formats
        ("text", "plain") => MimeType::TextPlain,
        ("text", "html") => MimeType::TextHtml,
        ("text", "css") => MimeType::TextCss,
        ("text", "javascript") | ("application", "javascript") => MimeType::TextJavascript,
        ("text", "csv") => MimeType::TextCsv,
        ("text", "markdown") | ("text", "md") => MimeType::TextMarkdown,

        // Audio formats
        ("audio", "mpeg") | ("audio", "mp3") => MimeType::AudioMpeg,
        ("audio", "wav") => MimeType::AudioWav,
        ("audio", "ogg") => MimeType::AudioOgg,
        ("audio", "flac") => MimeType::AudioFlac,

        // Video formats
        ("video", "mp4") => MimeType::VideoMp4,
        ("video", "webm") => MimeType::VideoWebm,
        ("video", "mpeg") => MimeType::VideoMpeg,
        ("video", "x-msvideo") | ("video", "avi") => MimeType::VideoAvi,

        // Application formats
        ("application", "json") => MimeType::ApplicationJson,
        ("application", "xml") | ("text", "xml") => MimeType::ApplicationXml,
        ("application", "zip") => MimeType::ApplicationZip,
        ("application", "gzip") | ("application", "x-gzip") => MimeType::ApplicationGzip,
        ("application", "x-tar") | ("application", "tar") => MimeType::ApplicationTar,

        // Font formats
        ("font", "woff") => MimeType::FontWoff,
        ("font", "woff2") => MimeType::FontWoff2,
        ("font", "ttf") | ("application", "font-sfnt") => MimeType::FontTtf,
        ("font", "otf") => MimeType::FontOtf,

        // Miscellaneous
        ("application", "rtf") => MimeType::ApplicationRtf,
        ("application", "sql") => MimeType::ApplicationSql,
        ("application", "x-yaml") | ("text", "yaml") => MimeType::ApplicationYaml,

        // Everything else -> mark as Custom
        _ => {
            let full_mime = mime.as_ref().to_ascii_lowercase();
            let mime_str = to_name(&full_mime);
            return (MimeType::Custom, mime_str);
        }
    };

    (code, empty)
}
