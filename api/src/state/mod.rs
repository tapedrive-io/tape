mod archive;
mod epoch;
mod spool;
mod tape;
mod treasury;
mod writer;
mod miner;

pub use archive::*;
pub use epoch::*;
pub use spool::*;
pub use tape::*;
pub use treasury::*;
pub use writer::*;
pub use miner::*;

use steel::*;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
pub enum AccountType {
    Unknown = 0,
    Archive,
    Spool,
    Writer,
    Tape,
    Miner,
    Epoch,
    Treasury,
}
