use steel::*;
use crate::consts::*;
use crate::utils::*;

#[derive(Debug, Clone, Copy)]
pub struct Chunk([u8; CHUNK_SIZE]);

impl Chunk {
    pub fn as_bytes(&self) -> &[u8; CHUNK_SIZE] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Segment([u8; SEGMENT_SIZE]);

impl Segment {
    pub fn try_from_bytes(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() > SEGMENT_SIZE {
            return Err(ProgramError::InvalidArgument);
        }
        Ok(Self(padded_array::<SEGMENT_SIZE>(data)))
    }

    pub fn chunks(&self) -> impl Iterator<Item = Chunk> + '_ {
        self.0
            .chunks(CHUNK_SIZE)
            .map(|chunk| Chunk(chunk.try_into().expect("Chunk size mismatch")))
    }
}
