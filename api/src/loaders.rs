use steel::*;

use crate::consts::*;
use crate::state::{Archive, Epoch, Treasury};

pub trait AccountInfoLoader {
    fn is_archive(&self) -> Result<&Self, ProgramError>;
    fn is_epoch(&self) -> Result<&Self, ProgramError>;
    fn is_treasury(&self) -> Result<&Self, ProgramError>;
    fn is_treasury_ata(&self) -> Result<&Self, ProgramError>;
    fn is_spool(&self) -> Result<&Self, ProgramError>;
}

impl AccountInfoLoader for AccountInfo<'_> {
    fn is_archive(&self) -> Result<&Self, ProgramError> {
        self.has_address(&ARCHIVE_ADDRESS)?
            .is_type::<Archive>(&crate::ID)
    }

    fn is_epoch(&self) -> Result<&Self, ProgramError> {
        self.has_address(&EPOCH_ADDRESS)?
            .is_type::<Epoch>(&crate::ID)
    }

    fn is_treasury(&self) -> Result<&Self, ProgramError> {
        self.has_address(&TREASURY_ADDRESS)?
            .is_type::<Treasury>(&crate::ID)
    }

    fn is_treasury_ata(&self) -> Result<&Self, ProgramError> {
        self.has_address(&TREASURY_ATA)
    }

    fn is_spool(&self) -> Result<&Self, ProgramError> {
        if !SPOOL_ADDRESSES.contains(self.key) {
            return Err(ProgramError::InvalidSeeds);
        }
        Ok(self)
    }
}
