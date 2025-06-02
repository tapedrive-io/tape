use array_const_fn_init::array_const_fn_init;
use const_crypto::ed25519;
use solana_program::pubkey::Pubkey;

pub const ARCHIVE: &[u8]                   = b"archive";
pub const EPOCH: &[u8]                     = b"epoch";
pub const TREASURY: &[u8]                  = b"treasury";
pub const SPOOL: &[u8]                     = b"spool";
pub const TAPE: &[u8]                      = b"tape";
pub const WRITER: &[u8]                    = b"writer";
pub const MINER: &[u8]                     = b"miner";

pub const MINT: &[u8]                      = b"mint";
pub const MINT_SEED: &[u8]                 = &[152, 68, 212, 200, 25, 113, 221, 71];

pub const METADATA: &[u8]                  = b"metadata";
pub const METADATA_NAME: &str              = "TAPE";
pub const METADATA_SYMBOL: &str            = "TAPE";
pub const METADATA_URI: &str               = "https://tapedrive.io/metadata.json";

pub const TREE_HEIGHT: usize               = 18;
pub const PROOF_LEN: usize                 = TREE_HEIGHT;

pub const SEGMENT_SIZE: usize              = 128; // Bytes (chosen to fit recall proofs comfortably)
pub const MAX_TAPE_SIZE: usize             = 2_usize.pow(TREE_HEIGHT as u32) * SEGMENT_SIZE; // 32MB

pub const SPOOL_COUNT: usize               = 8;
pub const NAME_LEN: usize                  = 32;  // Bytes
pub const HEADER_SIZE: usize               = 128; // Bytes

pub const TOKEN_DECIMALS: u8               = 10;
pub const ONE_TAPE: u64                    = 10u64.pow(TOKEN_DECIMALS as u32);
pub const MAX_SUPPLY: u64                  = 7_000_000 * ONE_TAPE;

pub const ONE_SECOND: i64                  = 1;
pub const ONE_MINUTE: i64                  = 60 * ONE_SECOND;
pub const EPOCH_DURATION_MINUTES: i64      = 15;
pub const EPOCH_SECONDS: i64               = EPOCH_DURATION_MINUTES * ONE_MINUTE;
pub const GRACE_PERIOD_SECONDS: i64        = 15 * ONE_SECOND;

// -- Const Addresses --
// (There isn't a better way to do this yet; maybe a build.rs + include)

pub const PROGRAM_ID: [u8; 32] = 
    unsafe { *(&crate::id() as *const Pubkey as *const [u8; 32]) };

pub const ARCHIVE_ADDRESS: Pubkey =
    Pubkey::new_from_array(ed25519::derive_program_address(&[ARCHIVE], &PROGRAM_ID).0);

pub const ARCHIVE_BUMP: u8 =
    ed25519::derive_program_address(&[ARCHIVE], &PROGRAM_ID).1;

pub const EPOCH_ADDRESS: Pubkey =
    Pubkey::new_from_array(ed25519::derive_program_address(&[EPOCH], &PROGRAM_ID).0);

pub const EPOCH_BUMP: u8 =
    ed25519::derive_program_address(&[EPOCH], &PROGRAM_ID).1;

pub const MINT_ADDRESS: Pubkey =
    Pubkey::new_from_array(ed25519::derive_program_address(&[MINT, &MINT_SEED], &PROGRAM_ID).0);

pub const MINT_BUMP: u8 = 
    ed25519::derive_program_address(&[MINT, &MINT_SEED], &PROGRAM_ID).1;

pub const TREASURY_ADDRESS: Pubkey =
    Pubkey::new_from_array(ed25519::derive_program_address(&[TREASURY], &PROGRAM_ID).0);

pub const TREASURY_BUMP: u8 = 
    ed25519::derive_program_address(&[TREASURY], &PROGRAM_ID).1;

pub const TREASURY_ATA: Pubkey = Pubkey::new_from_array(
    ed25519::derive_program_address(
        &[
            unsafe { &*(&TREASURY_ADDRESS as *const Pubkey as *const [u8; 32]) },
            unsafe { &*(&spl_token::id() as *const Pubkey as *const [u8; 32]) },
            unsafe { &*(&MINT_ADDRESS as *const Pubkey as *const [u8; 32]) },
        ],
        unsafe { &*(&spl_associated_token_account::id() as *const Pubkey as *const [u8; 32]) },
    )
    .0,
);

pub const SPOOL_ADDRESSES: [Pubkey; SPOOL_COUNT] = 
    array_const_fn_init![const_spool_address; 8];

const fn const_spool_address(i: usize) -> Pubkey {
    Pubkey::new_from_array(
        ed25519::derive_program_address(&[SPOOL, &[i as u8]], &PROGRAM_ID).0
    )
}
