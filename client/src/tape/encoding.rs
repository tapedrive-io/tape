use anyhow::{Result, anyhow};
use crate::utils::*;
use super::{TapeHeader, CompressionAlgo};

/// Encodes data into a tape format, applying compression if specified in the header.
pub fn encode_tape(data: &[u8], header: &TapeHeader) -> Result<Vec<u8>> {

    let compression_algo = CompressionAlgo::try_from(header.compression)
        .map_err(|_| anyhow!("Invalid compression algorithm"))? ;

    let compressed = match compression_algo {
        CompressionAlgo::None => Ok(data.to_vec()),
        CompressionAlgo::Gzip => compress(data),
    };

    // Add encryption, etc...

    compressed
}

/// Decodes a tape format into raw data, decompressing if necessary based on the header.
pub fn decode_tape(data: Vec<u8>, header: &TapeHeader) -> Result<Vec<u8>> {

    let compression_algo = CompressionAlgo::try_from(header.compression)
        .map_err(|_| anyhow!("Invalid compression algorithm"))?;

    let decompressed = match compression_algo {
        CompressionAlgo::None => Ok(data),
        CompressionAlgo::Gzip => decompress(&data),
    };

    // Add encryption, etc...

    decompressed
}
