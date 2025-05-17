use steel::*;

#[repr(u32)]
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq, IntoPrimitive)]
pub enum TapeError {
    #[error("Unknown error")]
    UnknownError = 0,
    #[error("The provided tape is in an unexpected state")]
    UnexpectedState = 1,
    #[error("The tape write failed")]
    WriteFailed = 2,
    #[error("The provided hash is invalid")]
    SolutionInvalid = 3,
    #[error("The provided hash did not satisfy the minimum required difficulty")]
    SolutionTooEasy = 4,
    #[error("You are trying to submit a solution too early")]
    SolutionTooEarly = 5,
    #[error("The provided claim is too large")]
    ClaimTooLarge = 6,
    #[error("The epoch has ended and needs to be advanced")]
    StaleEpoch = 7,
    #[error("The clock time is invalid")]
    ClockInvalid = 8,
    #[error("The maximum supply has been reached")]
    MaxSupply = 9,
}

error!(TapeError);
