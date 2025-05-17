use steel::*;
use super::AccountType;
use crate::state;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Epoch {
    pub number: u64,
    pub difficulty: u64,
    pub last_epoch_at: i64,
    pub base_rate: u64,
    pub target_rate: u64,
}

state!(AccountType, Epoch);
