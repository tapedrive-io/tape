use anyhow::{Result, anyhow};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use std::io::{Read, Write};
use tape_api::consts::*;

pub fn serialize<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    bincode::serialize(value).map_err(|e| anyhow!("Serialization failed: {}", e))
}

pub fn deserialize<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T> {
    bincode::deserialize(data).map_err(|e| anyhow!("Deserialization failed: {}", e))
}

pub fn compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish().map_err(Into::into)
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn estimate_chunks(data_len: usize) -> usize {
    data_len / SEGMENT_SIZE + if data_len % SEGMENT_SIZE != 0 { 1 } else { 0 }
}
