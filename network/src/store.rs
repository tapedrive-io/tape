use rocksdb::{ColumnFamilyDescriptor, DBCompressionType, Options, WriteBatch, DB};
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::path::Path;
use tape_api::SEGMENT_SIZE;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("RocksDB error: {0}")]
    RocksDB(#[from] rocksdb::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Health column family not found")]
    HealthCfNotFound,
    
    #[error("Tapes column family not found")]
    TapesCfNotFound,
    #[error("Segments column family not found")]
    SegmentsCfNotFound,
    #[error("Tape not found: number {0}")]
    TapeNotFound(u64),
    #[error("Segment not found for tape address {0}, segment {1}")]
    SegmentNotFound(String, u64),
    #[error("Tape not found for address: {0}")]
    TapeNotFoundForAddress(String),
    #[error("Invalid pubkey: {0}")]
    InvalidPubkey(String),
    #[error("Segment data exceeds maximum size of {0} bytes")]
    SegmentSizeExceeded(usize),
    #[error("Invalid segment key format")]
    InvalidSegmentKey,
    #[error("Invalid path")]
    InvalidPath,
}

pub struct TapeStore {
    db: DB,
}

impl TapeStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StoreError> {
        let path = path.as_ref();
        let mut cf_opts = Options::default();
        cf_opts.create_if_missing(true);
        cf_opts.set_compression_type(DBCompressionType::Lz4);

        let cf_tapes    = ColumnFamilyDescriptor::new("tapes", cf_opts.clone());
        let cf_segments = ColumnFamilyDescriptor::new("segments", cf_opts.clone());
        let cf_health   = ColumnFamilyDescriptor::new("health", cf_opts.clone());

        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        db_opts.set_write_buffer_size(128 * 1024 * 1024);
        db_opts.set_max_write_buffer_number(4);
        db_opts.create_missing_column_families(true);

        let db = DB::open_cf_descriptors(
            &db_opts,
            path,
            vec![cf_tapes, cf_segments, cf_health],
        )?;

        Ok(Self { db })
    }

    pub fn new_secondary<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
    ) -> Result<Self, StoreError> {
        let primary_path = primary_path.as_ref();
        let secondary_path = secondary_path.as_ref();
        let mut cf_opts = Options::default();
        cf_opts.set_compression_type(DBCompressionType::Lz4);

        let cf_tapes    = ColumnFamilyDescriptor::new("tapes", cf_opts.clone());
        let cf_segments = ColumnFamilyDescriptor::new("segments", cf_opts.clone());
        let cf_health   = ColumnFamilyDescriptor::new("health", cf_opts.clone());

        let mut db_opts = Options::default();
        db_opts.set_compression_type(DBCompressionType::Lz4);

        let db = DB::open_cf_descriptors_as_secondary(
            &db_opts,
            primary_path,
            secondary_path,
            vec![cf_tapes, cf_segments, cf_health],
        )?;
        Ok(Self { db })
    }

    pub fn catch_up_with_primary(&self) -> Result<(), StoreError> {
        self.db.try_catch_up_with_primary()?;
        Ok(())
    }

    /// Update the health values in the database.
    pub fn update_health(&self, last_processed_slot: u64, drift: u64) -> Result<(), StoreError> {
        let cf = self
            .db
            .cf_handle("health")
            .ok_or(StoreError::HealthCfNotFound)?;

        let mut batch = WriteBatch::default();
        batch.put_cf(cf, b"last_processed_slot", &last_processed_slot.to_be_bytes());
        batch.put_cf(cf, b"drift", &drift.to_be_bytes());

        self.db.write(batch)?;

        Ok(())
    }

    /// Load the last‐written health values.
    pub fn get_health(&self) -> Result<(u64, u64), StoreError> {
        let cf = self
            .db
            .cf_handle("health")
            .ok_or(StoreError::HealthCfNotFound)?;

        let bh = self
            .db
            .get_cf(cf, b"last_processed_slot")?
            .ok_or(StoreError::HealthCfNotFound)?;

        let dr = self
            .db
            .get_cf(cf, b"drift")?
            .ok_or(StoreError::HealthCfNotFound)?;

        let height = u64::from_be_bytes(bh[..].try_into().unwrap());
        let drift = u64::from_be_bytes(dr[..].try_into().unwrap());

        Ok((height, drift))
    }


    pub fn add_tape(&self, tape_number: u64, address: &Pubkey) -> Result<(), StoreError> {
        let cf_tapes = self
            .db
            .cf_handle("tapes")
            .ok_or(StoreError::TapesCfNotFound)?;

        let tape_number_key = tape_number.to_be_bytes().to_vec();
        let address_key = address.to_bytes().to_vec();

        let mut batch = WriteBatch::default();
        // Store tape_number -> address
        batch.put_cf(cf_tapes, tape_number_key, address.to_bytes());
        // Store address -> tape_number
        batch.put_cf(cf_tapes, address_key, tape_number.to_be_bytes());
        self.db.write(batch)?;

        Ok(())
    }

    pub fn add_segment(
        &self,
        tape_address: &Pubkey,
        segment_number: u64,
        data: Vec<u8>,
    ) -> Result<(), StoreError> {
        if data.len() > SEGMENT_SIZE {
            return Err(StoreError::SegmentSizeExceeded(SEGMENT_SIZE));
        }

        let cf_segments = self
            .db
            .cf_handle("segments")
            .ok_or(StoreError::SegmentsCfNotFound)?;

        // Create key: [<tape_address><segment_number>]
        let mut key = Vec::with_capacity(40); // 32 bytes for pubkey + 8 bytes for segment_number
        key.extend_from_slice(&tape_address.to_bytes());
        key.extend_from_slice(&segment_number.to_be_bytes());

        let mut batch = WriteBatch::default();
        batch.put_cf(cf_segments, &key, data);
        self.db.write(batch)?;

        Ok(())
    }

    pub fn get_tape_number(&self, address: &Pubkey) -> Result<u64, StoreError> {
        let cf_tapes = self
            .db
            .cf_handle("tapes")
            .ok_or(StoreError::TapesCfNotFound)?;

        let address_key = address.to_bytes().to_vec();
        let tape_number_bytes = self
            .db
            .get_cf(cf_tapes, &address_key)?
            .ok_or_else(|| StoreError::TapeNotFoundForAddress(address.to_string()))?;

        Ok(u64::from_be_bytes(
            tape_number_bytes
                .try_into()
                .map_err(|_| StoreError::InvalidSegmentKey)?,
        ))
    }

    pub fn get_tape_address(&self, tape_number: u64) -> Result<Pubkey, StoreError> {
        let cf_tapes = self
            .db
            .cf_handle("tapes")
            .ok_or(StoreError::TapesCfNotFound)?;

        let tape_number_key = tape_number.to_be_bytes().to_vec();
        let address_bytes = self
            .db
            .get_cf(cf_tapes, &tape_number_key)?
            .ok_or(StoreError::TapeNotFound(tape_number))?;

        Pubkey::try_from(address_bytes.as_slice())
            .map_err(|e| StoreError::InvalidPubkey(e.to_string()))
    }

    pub fn get_tape_segments(
        &self,
        tape_address: &Pubkey,
    ) -> Result<Vec<(u64, Vec<u8>)>, StoreError> {
        let cf_segments = self
            .db
            .cf_handle("segments")
            .ok_or(StoreError::SegmentsCfNotFound)?;

        let mut segments = Vec::new();
        let prefix = tape_address.to_bytes().to_vec();

        let iter = self.db.prefix_iterator_cf(cf_segments, &prefix);
        for item in iter {
            let (key, value) = item?;
            if key.len() != 40 {
                // 32 bytes for pubkey + 8 bytes for segment_number
                continue;
            }
            if !key.starts_with(&prefix) {
                continue;
            }

            let segment_number = u64::from_be_bytes(
                key[32..40]
                    .try_into()
                    .map_err(|_| StoreError::InvalidSegmentKey)?,
            );

            segments.push((segment_number, value.to_vec()));
        }

        // Sort by segment_number
        segments.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(segments)
    }

    pub fn get_segment(
        &self,
        tape_address: &Pubkey,
        segment_number: u64,
    ) -> Result<Vec<u8>, StoreError> {
        let cf_segments = self
            .db
            .cf_handle("segments")
            .ok_or(StoreError::SegmentsCfNotFound)?;

        let mut key = Vec::with_capacity(40); // 32 bytes for pubkey + 8 bytes for segment_number
        key.extend_from_slice(&tape_address.to_bytes());
        key.extend_from_slice(&segment_number.to_be_bytes());

        let segment_data = self
            .db
            .get_cf(cf_segments, &key)?
            .ok_or_else(|| StoreError::SegmentNotFound(tape_address.to_string(), segment_number))?;

        Ok(segment_data.to_vec())
    }
}

impl Drop for TapeStore {
    fn drop(&mut self) {
        // RocksDB handles cleanup automatically
    }
}

pub fn primary() -> Result<TapeStore, StoreError> {
    let current_dir = env::current_dir().map_err(|e| StoreError::IoError(e))?;
    let db_primary = current_dir.join("db_tapestore");
    std::fs::create_dir_all(&db_primary).map_err(|e| StoreError::IoError(e))?;
    TapeStore::new(&db_primary)
}

pub fn secondary() -> Result<TapeStore, StoreError> {
    let current_dir = env::current_dir().map_err(|e| StoreError::IoError(e))?;
    let db_primary = current_dir.join("db_tapestore");
    let db_secondary = current_dir.join("db_tapestore_read");
    std::fs::create_dir_all(&db_secondary).map_err(|e| StoreError::IoError(e))?;
    TapeStore::new_secondary(&db_primary, &db_secondary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use tempdir::TempDir;

    fn setup_store() -> Result<(TapeStore, TempDir), StoreError> {
        let temp_dir = TempDir::new("rocksdb_test").map_err(StoreError::IoError)?;
        let store = TapeStore::new(temp_dir.path())?;
        Ok((store, temp_dir))
    }

    #[test]
    fn test_add_and_get_tape() -> Result<(), StoreError> {
        let (store, _temp_dir) = setup_store()?;
        let tape_number = 1;
        let address = Pubkey::new_unique();

        store.add_tape(tape_number, &address)?;
        let retrieved_number = store.get_tape_number(&address)?;
        assert_eq!(retrieved_number, tape_number);
        let retrieved_address = store.get_tape_address(tape_number)?;
        assert_eq!(retrieved_address, address);

        Ok(())
    }

    #[test]
    fn test_add_and_get_segments() -> Result<(), StoreError> {
        let (store, _temp_dir) = setup_store()?;
        let tape_number = 1;
        let address = Pubkey::new_unique();

        store.add_tape(tape_number, &address)?;
        let segment_data_1 = vec![1, 2, 3];
        let segment_data_2 = vec![4, 5, 6];
        store.add_segment(&address, 0, segment_data_1.clone())?;
        store.add_segment(&address, 1, segment_data_2.clone())?;

        let segments = store.get_tape_segments(&address)?;
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], (0, segment_data_1));
        assert_eq!(segments[1], (1, segment_data_2));

        let segments = store.get_tape_segments(&Pubkey::new_unique())?;
        assert_eq!(segments.len(), 0);

        Ok(())
    }

    #[test]
    fn test_segment_size_limit() -> Result<(), StoreError> {
        let (store, _temp_dir) = setup_store()?;
        let address = Pubkey::new_unique();

        let oversized_data = vec![0; SEGMENT_SIZE + 1];
        let result = store.add_segment(&address, 0, oversized_data);
        assert!(matches!(result, Err(StoreError::SegmentSizeExceeded(_))));

        Ok(())
    }

    #[test]
    fn test_error_cases() -> Result<(), StoreError> {
        let (store, _temp_dir) = setup_store()?;
        let address = Pubkey::new_unique();

        let result = store.get_tape_number(&address);
        assert!(matches!(result, Err(StoreError::TapeNotFoundForAddress(_))));

        let result = store.get_tape_address(1);
        assert!(matches!(result, Err(StoreError::TapeNotFound(1))));

        Ok(())
    }

    #[test]
    fn test_multiple_tapes() -> Result<(), StoreError> {
        let (store, _temp_dir) = setup_store()?;

        let tape1_number = 1;
        let tape1_address = Pubkey::new_unique();
        let tape2_number = 2;
        let tape2_address = Pubkey::new_unique();

        store.add_tape(tape1_number, &tape1_address)?;
        store.add_tape(tape2_number, &tape2_address)?;
        store.add_segment(&tape1_address, 0, vec![1, 2, 3])?;
        store.add_segment(&tape2_address, 0, vec![4, 5, 6])?;

        assert_eq!(store.get_tape_number(&tape1_address)?, tape1_number);
        assert_eq!(store.get_tape_address(tape1_number)?, tape1_address);
        let tape1_segments = store.get_tape_segments(&tape1_address)?;
        assert_eq!(tape1_segments.len(), 1);
        assert_eq!(tape1_segments[0], (0, vec![1, 2, 3]));

        assert_eq!(store.get_tape_number(&tape2_address)?, tape2_number);
        assert_eq!(store.get_tape_address(tape2_number)?, tape2_address);
        let tape2_segments = store.get_tape_segments(&tape2_address)?;
        assert_eq!(tape2_segments.len(), 1);
        assert_eq!(tape2_segments[0], (0, vec![4, 5, 6]));

        Ok(())
    }

    #[test]
    fn test_get_segment() -> Result<(), StoreError> {
        let (store, _temp_dir) = setup_store()?;
        let tape_number = 1;
        let segment_number = 0;
        let address = Pubkey::new_unique();
        let segment_data = vec![1, 2, 3];

        store.add_tape(tape_number, &address)?;
        store.add_segment(&address, segment_number, segment_data.clone())?;

        let retrieved_data = store.get_segment(&address, segment_number)?;
        assert_eq!(retrieved_data, segment_data);

        Ok(())
    }

    #[test]
    fn test_get_segment_non_existent_segment() -> Result<(), StoreError> {
        let (store, _temp_dir) = setup_store()?;
        let address = Pubkey::new_unique();
        let segment_number = 0;

        let result = store.get_segment(&address, segment_number);
        assert!(matches!(result, Err(StoreError::SegmentNotFound(_, s)) if s == segment_number));

        Ok(())
    }

    #[test]
    fn test_get_multiple_segments() -> Result<(), StoreError> {
        let (store, _temp_dir) = setup_store()?;
        let tape_number = 1;
        let address = Pubkey::new_unique();
        let segment_data_1 = vec![1, 2, 3];
        let segment_data_2 = vec![4, 5, 6];

        store.add_tape(tape_number, &address)?;
        store.add_segment(&address, 0, segment_data_1.clone())?;
        store.add_segment(&address, 1, segment_data_2.clone())?;

        let retrieved_data_1 = store.get_segment(&address, 0)?;
        assert_eq!(retrieved_data_1, segment_data_1);

        let retrieved_data_2 = store.get_segment(&address, 1)?;
        assert_eq!(retrieved_data_2, segment_data_2);

        Ok(())
    }
}
