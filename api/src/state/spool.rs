use steel::*;
use super::AccountType;
use crate::state;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Spool {
    pub id: u64,
    pub available_rewards: u64,
    pub theoretical_rewards: u64,
}

state!(AccountType, Spool);
