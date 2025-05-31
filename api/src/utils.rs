use steel::*;
use crate::consts::*;
use crate::error::*;
use brine_tree::{MerkleTree, Leaf};

/// Helper: check a condition is true and return an error if not
pub fn check_condition<E>(condition: bool, err: E) -> ProgramResult
where
    E: Into<ProgramError>,
{
    if !condition {
        return Err(err.into());
    }
    Ok(())
}

/// Helper: convert a slice to a fixed-size array, truncating or padding with zeros as needed
pub fn padded_array<const N: usize>(input: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    let len = input.len().min(N);
    out[..len].copy_from_slice(&input[..len]);
    out
}

/// Helper: convert a name to a fixed-size array
pub fn to_name(val: &str) -> [u8; MAX_NAME_LEN] {
    assert!(val.len() <= MAX_NAME_LEN, "name too long");
    padded_array::<MAX_NAME_LEN>(val.as_bytes())
}

/// Helper: convert a name to a string
pub fn from_name(val: &[u8; MAX_NAME_LEN]) -> String {
    let mut name_bytes = val.to_vec();
    name_bytes.retain(|&x| x != 0);
    String::from_utf8(name_bytes).unwrap()
}

/// Helper: compute a leaf from a segment id and segment data
#[inline(always)]
pub fn compute_leaf(
    segment_id: u64, 
    segment: &[u8; SEGMENT_SIZE],
) -> Leaf {
    let segment_id = segment_id.to_le_bytes();

    Leaf::new(&[
        segment_id.as_ref(), // u64 (8 bytes)
        segment,
    ])
}

/// Helper: write chunks to the Merkle tree
#[inline(always)]
pub fn write_segment(
    tree: &mut MerkleTree<{TREE_HEIGHT}>,
    segment_id: u64,
    segment: &[u8; SEGMENT_SIZE],
) -> ProgramResult {

    let leaf = compute_leaf(
        segment_id, 
        &segment);

    check_condition(
        tree.try_add_leaf(leaf).is_ok(),
        TapeError::WriteFailed,
    )?;

    Ok(())
}

// Helper: compute the recall tape number from a given challenge
#[inline(always)]
pub fn compute_recall_tape(
    challenge: &[u8; 32],
    total_tapes: u64,
) -> u64 {
    // Prevent division by zero
    if total_tapes == 0 {
        return 1;
    }

    // Compute the tape number from the challenge, tape 0 
    // is invalid and reprseents no tape
    (u64::from_le_bytes(challenge[0..8].try_into().unwrap()) % total_tapes)
        .max(1)
}

// Helper: compute the recall segment number from a given challenge
#[inline(always)]
pub fn compute_recall_segment(
    challenge: &[u8; 32],
    total_segments: u64,
) -> u64 {
    // Prevent division by zero
    if total_segments == 0 {
        return 0;
    }

    u64::from_le_bytes(challenge[8..16].try_into().unwrap()) % total_segments
}
