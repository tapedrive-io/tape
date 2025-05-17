use steel::*;
use super::AccountType;
use crate::consts::*;
use crate::state;

#[repr(C, align(8))] 
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Tape {
    pub number: u64,
    pub state: u64,

    pub authority: Pubkey,
    pub name: [u8; MAX_NAME_LEN],

    pub merkle_seed: [u8; 32],
    pub merkle_root: [u8; 32],
    pub tail: [u8; 64],

    pub total_segments: u64,
    pub total_size: u64,
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
pub enum TapeState {
    Unknown = 0,
    Created,
    Writing,
    Finalized,
}

state!(AccountType, Tape);
