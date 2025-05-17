mod read;
mod write;

pub use read::*;
pub use write::*;

use num_enum::*;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
pub enum TapeLayout {
    Raw        = 0,
    Compressed = 1,
}
