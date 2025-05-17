use steel::*;
use core::marker::PhantomData;
use crate::consts::*;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, TryFromPrimitive)]
pub enum InstructionType {
    Unknown = 0,
    Initialize,
    Advance,

    // Tape instructions
    Create,
    Write,
    Finalize,

    // Miner instructions
    Register,
    Close,
    Mine,
    Claim,
}

instruction!(InstructionType, Initialize);
instruction!(InstructionType, Advance);

instruction!(InstructionType, Create);
instruction!(InstructionType, Write);
instruction!(InstructionType, Finalize);

instruction!(InstructionType, Register);
instruction!(InstructionType, Close);
instruction!(InstructionType, Mine);
instruction!(InstructionType, Claim);


#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Initialize {}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Advance {}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Create {
    pub name: [u8; MAX_NAME_LEN],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Write {
    pub prev_segment: [u8; 64],
    _data: PhantomData<[u8]>,
}

impl Write {
    pub fn new(
        prev_segment: [u8; 64],
    ) -> Self {
        Self {
            prev_segment,
            _data: PhantomData,
        }
    }

    pub fn pack(&self, data: &[u8]) -> Vec<u8> {
        let discriminator = InstructionType::Write as u8;
        let mut result = Vec::with_capacity(1 + data.len());
        result.push(discriminator);
        result.extend_from_slice(&self.prev_segment);
        result.extend_from_slice(data);
        result
    }
}

pub struct ParsedWrite {
    pub prev_segment: [u8; 64],
    pub data: Vec<u8>,
}

impl ParsedWrite {
    pub fn try_from_bytes(data: &[u8]) -> Result<Self, ProgramError> {
        let prev_segment = data[0..64].try_into().unwrap();
        let data = data[64..].to_vec();

        Ok(Self {
            prev_segment,
            data,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Finalize {
    pub tail: [u8; 64],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Register {
    pub name: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Close {}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Mine {
    pub digest: [u8; 16],
    pub nonce: [u8; 8],
    pub recall_chunk: [u8; CHUNK_SIZE],
    pub recall_proof: [[u8; 32]; PROOF_LEN],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Claim {
    pub amount: [u8; 8],
}
