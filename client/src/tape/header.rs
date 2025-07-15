use anyhow::{Result, anyhow, bail};
use bytemuck::{Pod, Zeroable};
use num_enum::{IntoPrimitive, TryFromPrimitive};

/// A 4-byte "magic" prefix to identify the header format.
pub const HEADER_MAGIC: [u8; 4] = *b"TAPE";

/// The version of the header format.
pub const HEADER_VERSION: u8 = 1;

/// How many bytes to reserve for an ASCII "fallback" MIME‐string.
/// If `mime_type == MimeType::Custom as u8`, then `mime_str` holds the real text.
pub const MIME_STR_LEN: usize = 32;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
/// Predefined MIME types (u8). 256 slots; reserve 0xFF for "Custom" if needed.
/// Maps to common MIME types for efficient storage and processing.
pub enum MimeType {
    /// Unknown or generic binary data
    Unknown                = 0,   // application/octet-stream

    // Image formats
    ImagePng               = 1,   // image/png
    ImageJpeg              = 2,   // image/jpeg
    ImageGif               = 3,   // image/gif
    ImageWebp              = 4,   // image/webp
    ImageBmp               = 5,   // image/bmp
    ImageTiff              = 6,   // image/tiff

    // Document formats
    ApplicationPdf         = 10,  // application/pdf
    ApplicationMsword      = 11,  // application/msword
    ApplicationDocx        = 12,  // application/vnd.openxml...
    ApplicationOdt         = 13,  // application/vnd.oasis...

    // Text formats
    TextPlain              = 20,  // text/plain
    TextHtml               = 21,  // text/html
    TextCss                = 22,  // text/css
    TextJavascript         = 23,  // text/javascript
    TextCsv                = 24,  // text/csv
    TextMarkdown           = 25,  // text/markdown

    // Audio formats
    AudioMpeg              = 30,  // audio/mpeg
    AudioWav               = 31,  // audio/wav
    AudioOgg               = 32,  // audio/ogg
    AudioFlac              = 33,  // audio/flac

    // Video formats
    VideoMp4               = 40,  // video/mp4
    VideoWebm              = 41,  // video/webm
    VideoMpeg              = 42,  // video/mpeg
    VideoAvi               = 43,  // video/x-msvideo

    // Application formats
    ApplicationJson        = 50,  // application/json
    ApplicationXml         = 51,  // application/xml
    ApplicationZip         = 52,  // application/zip
    ApplicationGzip        = 53,  // application/gzip
    ApplicationTar         = 54,  // application/x-tar

    // Font formats
    FontWoff               = 60,  // font/woff
    FontWoff2              = 61,  // font/woff2
    FontTtf                = 62,  // font/ttf
    FontOtf                = 63,  // font/otf

    // Miscellaneous
    ApplicationRtf         = 70,  // application/rtf
    ApplicationSql         = 71,  // application/sql
    ApplicationYaml        = 72,  // application/x-yaml

    // Reserved for custom or user-defined MIME types
    Custom                 = 255, // Reserved for custom MIME types
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
/// Compression algorithm used on the payload (if any).
pub enum CompressionAlgo {
    None   = 0,
    Gzip   = 1,

    // Extend as needed...
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
/// The encryption algorithm used on the payload (if any).
pub enum EncryptionAlgo {
    None               = 0,

    // Extend as needed...
}

/// Tape data header. Note, none of the fields are verified onchain, they are 
/// opaque to the onchain program logic.
///
/// Layout:
/// - `magic` (4 bytes)           -> always `b"TAPE"`
/// - `version` (1 byte)          -> format version (`1`)
/// - `mime_type` (1 byte)        -> see `MimeType`
/// - `mime_str` (32 bytes)       -> null‐padded ASCII MIME string
/// - `compression` (1 byte)      -> see `CompressionAlgo`
/// - `encryption_algo` (1 byte)  -> see `EncryptionAlgo`
/// - `iv` (12 bytes)             -> IV/nonce if encrypted; all zeros otherwise
/// - `_unused` (76 bytes)        -> padding for 128-byte size
#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct TapeHeader {
    /// Fixed “magic” string. Readers should verify this equals `HEADER_MAGIC`.
    pub magic: [u8; 4],

    /// Version of this header format. Readers should verify matches `HEADER_VERSION`.
    pub version: u8,

    /// Predefined MIME type code (or `Custom` = 255 if you want to overload externally).
    pub mime_type: u8,

    /// If `mime_type == MimeType::Custom`, this contains a null‐padded ASCII MIME string.
    /// Otherwise, it’s all zero, and readers ignore it.
    pub mime_str: [u8; MIME_STR_LEN],

    /// Compression algorithm used (or `None`).
    pub compression: u8,

    /// Encryption algorithm used (or `None`).
    pub encryption_algo: u8,

    /// Initialization Vector (nonce) for the chosen encryption algorithm.
    /// If `encryption_algo == None`, this should be all zeros.
    pub iv: [u8; 12],

    _unused: [u8; 76], // future use
}

unsafe impl Zeroable for TapeHeader {}
unsafe impl Pod for TapeHeader {}

impl TapeHeader {
    pub fn new(
        mime_type: MimeType,
        compression: CompressionAlgo,
        encryption_algo: EncryptionAlgo,
    ) -> Self {
        assert!(
            mime_type != MimeType::Custom, 
            "Use custom MIME type only if you provide a valid `mime_str`."
        );

        Self {
            magic            : HEADER_MAGIC,
            version          : HEADER_VERSION,
            mime_type        : mime_type.into(),
            mime_str         : [0; MIME_STR_LEN],
            compression      : compression.into(),
            encryption_algo  : encryption_algo.into(),

            iv               : [0; 12], // empty IV/nonce
            _unused          : [0; 76],
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bytemuck::bytes_of(self).to_vec()
    }

    pub fn try_from_bytes(data: &[u8]) -> Result<&Self> {

        // Ensure we have at least 128 bytes.
        if data.len() < std::mem::size_of::<Self>() {
            bail!("Data too short for TapeHeader ({} < {})",
                  data.len(), std::mem::size_of::<Self>());
        }

        // Check the magic prefix.
        if &data[0..4] != HEADER_MAGIC {
            bail!("Invalid magic number in TapeHeader");
        }

        // Check the version byte.
        if data[4] != HEADER_VERSION {
            bail!(
                "Unsupported TapeHeader version: found {}, expected {}",
                data[4], HEADER_VERSION
            );
        }

        // Finally, try to cast via bytemuck.
        let header: &Self = bytemuck::try_from_bytes(data)
            .map_err(|e| anyhow!("Failed to cast bytes to TapeHeader: {}", e))?;

        Ok(header)
    }
}

impl std::fmt::Debug for TapeHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapeHeader")
            .field("version", &self.version)
            .field("mime_type", &self.mime_type)
            .field("mime_str", &String::from_utf8_lossy(&self.mime_str))
            .field("compression", &self.compression)
            .field("encryption_algo", &self.encryption_algo)
            .field("iv", &self.iv)
            .finish()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tape_header_creation() {
        let header = TapeHeader::new(
            MimeType::ImagePng,
            CompressionAlgo::None,
            EncryptionAlgo::None,
        );

        assert_eq!(header.magic, HEADER_MAGIC);
        assert_eq!(header.version, HEADER_VERSION);
        assert_eq!(header.mime_type, MimeType::ImagePng as u8);
        assert_eq!(header.compression, CompressionAlgo::None as u8);
        assert_eq!(header.encryption_algo, EncryptionAlgo::None as u8);
        assert_eq!(header.iv, [0; 12]);
    }

    #[test]
    fn test_tape_header_to_bytes() {
        let header = TapeHeader::new(
            MimeType::TextPlain,
            CompressionAlgo::Gzip,
            EncryptionAlgo::None,
        );

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), std::mem::size_of::<TapeHeader>());
    }
}
