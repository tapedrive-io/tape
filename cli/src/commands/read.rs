use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use num_enum::TryFromPrimitive;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::{ fs, io::{self, Write}, path::Path, str::FromStr };
use tokio::{task, time::Duration};

use crate::cli::{Cli, Commands};
use crate::log;
use tape_client::{ MimeType, decode_tape, get_tape_account, read_from_block, TapeHeader};

pub async fn handle_read_command(cli: Cli, client: RpcClient) -> Result<()> {
    if let Commands::Read { tape, output } = cli.command {
        let tape_address = Pubkey::from_str(&tape)
            .map_err(|_| anyhow::anyhow!("Invalid tape address: {}", tape))?;

        log::print_message("Reading tape...");
        log::print_divider();

        let pb = setup_progress_bar();

        pb.set_message("Fetching tape metadata...");
        let (tape, _) = get_tape_account(&client, &tape_address).await?;
        let header = TapeHeader::try_from_bytes(&tape.header)?;

        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.white/gray}] {pos}/{len} {wide_msg}")
                .expect("Failed to set progress style"),
        );
        pb.set_length(tape.total_segments);
        pb.set_position(0);
        pb.set_message("");

        let data = read_from_block(&client, &tape_address, tape.tail_slot).await?;
        let result = decode_tape(data, header)?;

        let mime_type_enum =
            MimeType::try_from_primitive(header.mime_type).unwrap_or(MimeType::Unknown);

        write_output(output, &result, mime_type_enum)?;
        log::print_divider();
    }
    Ok(())
}

fn setup_progress_bar() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
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
    pb
}

fn write_output(output: Option<String>, data: &[u8], mime_type: MimeType) -> Result<()> {
    match output {
        Some(mut filename) => {
            if Path::new(&filename).extension().is_none() {
                if let Some(ext) = get_extension(mime_type) {
                    filename.push('.');
                    filename.push_str(ext);
                }
            }
            fs::write(&filename, data)?;
            log::print_message(&format!("Wrote output to: {}", filename));
        }
        None => {
            io::stdout().write_all(data)?;
            io::stdout().flush()?;
        }
    }
    Ok(())
}

fn get_extension(mime_type: MimeType) -> Option<&'static str> {
    match mime_type {
        // Image formats
        MimeType::ImagePng => Some("png"),
        MimeType::ImageJpeg => Some("jpg"),
        MimeType::ImageGif => Some("gif"),
        MimeType::ImageWebp => Some("webp"),
        MimeType::ImageBmp => Some("bmp"),
        MimeType::ImageTiff => Some("tiff"),

        // Document formats
        MimeType::ApplicationPdf => Some("pdf"),
        MimeType::ApplicationMsword => Some("doc"),
        MimeType::ApplicationDocx => Some("docx"),
        MimeType::ApplicationOdt => Some("odt"),

        // Text formats
        MimeType::TextPlain => Some("txt"),
        MimeType::TextHtml => Some("html"),
        MimeType::TextCss => Some("css"),
        MimeType::TextJavascript => Some("js"),
        MimeType::TextCsv => Some("csv"),
        MimeType::TextMarkdown => Some("md"),

        // Audio formats
        MimeType::AudioMpeg => Some("mp3"),
        MimeType::AudioWav => Some("wav"),
        MimeType::AudioOgg => Some("ogg"),
        MimeType::AudioFlac => Some("flac"),

        // Video formats
        MimeType::VideoMp4 => Some("mp4"),
        MimeType::VideoWebm => Some("webm"),
        MimeType::VideoMpeg => Some("mpeg"),
        MimeType::VideoAvi => Some("avi"),

        // Application formats
        MimeType::ApplicationJson => Some("json"),
        MimeType::ApplicationXml => Some("xml"),
        MimeType::ApplicationZip => Some("zip"),
        MimeType::ApplicationGzip => Some("gz"),
        MimeType::ApplicationTar => Some("tar"),

        // Font formats
        MimeType::FontWoff => Some("woff"),
        MimeType::FontWoff2 => Some("woff2"),
        MimeType::FontTtf => Some("ttf"),
        MimeType::FontOtf => Some("otf"),

        // Miscellaneous
        MimeType::ApplicationRtf => Some("rtf"),
        MimeType::ApplicationSql => Some("sql"),
        MimeType::ApplicationYaml => Some("yaml"),

        MimeType::Unknown => Some("bin"),
    }
}
