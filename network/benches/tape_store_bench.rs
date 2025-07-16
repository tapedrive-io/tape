use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::{distributions::Alphanumeric, Rng};
use solana_sdk::pubkey::Pubkey;
use tape_api::prelude::*;
use tape_network::store::TapeStore;
use tempdir::TempDir;

const SEGMENTS_PER_TAPE: u64 = 1000;
const NUM_TAPES: usize = 100;

fn generate_random_data(size: usize) -> Vec<u8> {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(size)
        .collect()
}

fn bench_add_mutable_segments(c: &mut Criterion) {
    let temp_dir = TempDir::new("bench_add_mutable").unwrap();
    let store = TapeStore::new(temp_dir.path()).unwrap();

    let mut group = c.benchmark_group("add_mutable_segments");
    group.bench_function("add_mutable_segment", |b| {
        let tape_address = Pubkey::new_unique();
        let segment_number = 0;
        let data = generate_random_data(SEGMENT_SIZE);

        b.iter(|| {
            store
                .add_mutable_segment(
                    black_box(&tape_address),
                    black_box(segment_number),
                    black_box(data.clone()),
                )
                .unwrap();
        })
    });
    group.finish();
}

fn bench_add_mutable_slots(c: &mut Criterion) {
    let temp_dir = TempDir::new("bench_add_mutable_slots").unwrap();
    let store = TapeStore::new(temp_dir.path()).unwrap();

    let mut group = c.benchmark_group("add_mutable_slots");
    group.bench_function("add_mutable_slot", |b| {
        let tape_address = Pubkey::new_unique();
        let segment_number = 0;
        let slot = 12345;

        b.iter(|| {
            store
                .add_mutable_slot(
                    black_box(&tape_address),
                    black_box(segment_number),
                    black_box(slot),
                )
                .unwrap();
        })
    });
    group.finish();
}

fn bench_finalize_tape(c: &mut Criterion) {
    let mut group = c.benchmark_group("finalize_tape");

    group.bench_function("finalize_tape_with_segments", |b| {
        b.iter(|| {
            let temp_dir = TempDir::new("bench_finalize").unwrap();
            let store = TapeStore::new(temp_dir.path()).unwrap();
            let tape_address = Pubkey::new_unique();
            let tape_number = 1;

            for segment_number in 0..SEGMENTS_PER_TAPE {
                let data = generate_random_data(SEGMENT_SIZE);
                store
                    .add_mutable_segment(&tape_address, segment_number, data)
                    .unwrap();
                store
                    .add_mutable_slot(&tape_address, segment_number, segment_number)
                    .unwrap();
            }

            store
                .finalize_tape(black_box(&tape_address), black_box(tape_number))
                .unwrap();
        })
    });
    group.finish();
}

fn bench_finalize_many_tapes(c: &mut Criterion) {
    let mut group = c.benchmark_group("finalize_many_tapes");

    group.bench_function("finalize_many_tapes", |b| {
        b.iter(|| {
            let temp_dir = TempDir::new("bench_finalize_many").unwrap();
            let store = TapeStore::new(temp_dir.path()).unwrap();

            for tape_idx in 0..NUM_TAPES {
                let tape_address = Pubkey::new_unique();
                let tape_number = (tape_idx + 1) as u64;

                for segment_number in 0..SEGMENTS_PER_TAPE {
                    let data = generate_random_data(SEGMENT_SIZE);
                    store
                        .add_mutable_segment(&tape_address, segment_number, data)
                        .unwrap();
                    store
                        .add_mutable_slot(&tape_address, segment_number, segment_number)
                        .unwrap();
                }

                store
                    .finalize_tape(black_box(&tape_address), black_box(tape_number))
                    .unwrap();
            }
        })
    });
    group.finish();
}

fn bench_get_segment(c: &mut Criterion) {
    let temp_dir = TempDir::new("bench_get_segment").unwrap();
    let store = TapeStore::new(temp_dir.path()).unwrap();

    let mut tape_numbers = Vec::with_capacity(NUM_TAPES);
    for tape_idx in 0..NUM_TAPES {
        let tape_address = Pubkey::new_unique();
        let tape_number = (tape_idx + 1) as u64;
        tape_numbers.push(tape_number);

        for segment_number in 0..SEGMENTS_PER_TAPE {
            let data = generate_random_data(SEGMENT_SIZE);
            store
                .add_mutable_segment(&tape_address, segment_number, data)
                .unwrap();
            store
                .add_mutable_slot(&tape_address, segment_number, segment_number)
                .unwrap();
        }
        store.finalize_tape(&tape_address, tape_number).unwrap();
    }

    let mut group = c.benchmark_group("get_segment");
    group.bench_function("get_segment_many_tapes", |b| {
        let tape_number = tape_numbers[NUM_TAPES / 2];
        let segment_number = SEGMENTS_PER_TAPE / 2;

        b.iter(|| {
            store
                .get_segment(black_box(tape_number), black_box(segment_number))
                .unwrap();
        })
    });
    group.finish();
}

fn bench_get_mutable_segment(c: &mut Criterion) {
    let temp_dir = TempDir::new("bench_get_mutable_segment").unwrap();
    let store = TapeStore::new(temp_dir.path()).unwrap();

    let mut tape_addresses = Vec::with_capacity(NUM_TAPES);
    for _tape_idx in 0..NUM_TAPES {
        let tape_address = Pubkey::new_unique();
        tape_addresses.push(tape_address);

        for segment_number in 0..SEGMENTS_PER_TAPE {
            let data = generate_random_data(SEGMENT_SIZE);
            store
                .add_mutable_segment(&tape_address, segment_number, data)
                .unwrap();
            store
                .add_mutable_slot(&tape_address, segment_number, segment_number)
                .unwrap();
        }
    }

    let mut group = c.benchmark_group("get_mutable_segment");
    group.bench_function("get_mutable_segment_many_tapes", |b| {
        let tape_address = tape_addresses[NUM_TAPES / 2];
        let segment_number = SEGMENTS_PER_TAPE / 2;

        b.iter(|| {
            store
                .get_mutable_segment(black_box(&tape_address), black_box(segment_number))
                .unwrap();
        })
    });
    group.finish();
}

fn bench_get_tape_segments(c: &mut Criterion) {
    let temp_dir = TempDir::new("bench_get_tape_segments").unwrap();
    let store = TapeStore::new(temp_dir.path()).unwrap();

    let mut tape_numbers = Vec::with_capacity(NUM_TAPES);
    for tape_idx in 0..NUM_TAPES {
        let tape_address = Pubkey::new_unique();
        let tape_number = (tape_idx + 1) as u64;
        tape_numbers.push(tape_number);

        for segment_number in 0..SEGMENTS_PER_TAPE {
            let data = generate_random_data(SEGMENT_SIZE);
            store
                .add_mutable_segment(&tape_address, segment_number, data)
                .unwrap();
            store
                .add_mutable_slot(&tape_address, segment_number, segment_number)
                .unwrap();
        }
        store.finalize_tape(&tape_address, tape_number).unwrap();
    }

    let mut group = c.benchmark_group("get_tape_segments");
    group.bench_function("get_tape_segments_many_tapes", |b| {
        let tape_number = tape_numbers[NUM_TAPES / 2];

        b.iter(|| {
            store.get_tape_segments(black_box(tape_number)).unwrap();
        })
    });
    group.finish();
}

fn bench_get_slot(c: &mut Criterion) {
    let temp_dir = TempDir::new("bench_get_slot").unwrap();
    let store = TapeStore::new(temp_dir.path()).unwrap();

    let mut tape_numbers = Vec::with_capacity(NUM_TAPES);
    for tape_idx in 0..NUM_TAPES {
        let tape_address = Pubkey::new_unique();
        let tape_number = (tape_idx + 1) as u64;
        tape_numbers.push(tape_number);

        for segment_number in 0..SEGMENTS_PER_TAPE {
            let data = generate_random_data(SEGMENT_SIZE);
            store
                .add_mutable_segment(&tape_address, segment_number, data)
                .unwrap();
            store
                .add_mutable_slot(&tape_address, segment_number, segment_number)
                .unwrap();
        }
        store.finalize_tape(&tape_address, tape_number).unwrap();
    }

    let mut group = c.benchmark_group("get_slot");
    group.bench_function("get_slot_many_tapes", |b| {
        let tape_number = tape_numbers[NUM_TAPES / 2];
        let segment_number = SEGMENTS_PER_TAPE / 2;

        b.iter(|| {
            store
                .get_slot(black_box(tape_number), black_box(segment_number))
                .unwrap();
        })
    });
    group.finish();
}

fn bench_get_mutable_slot(c: &mut Criterion) {
    let temp_dir = TempDir::new("bench_get_mutable_slot").unwrap();
    let store = TapeStore::new(temp_dir.path()).unwrap();

    let mut tape_addresses = Vec::with_capacity(NUM_TAPES);
    for _tape_idx in 0..NUM_TAPES {
        let tape_address = Pubkey::new_unique();
        tape_addresses.push(tape_address);

        for segment_number in 0..SEGMENTS_PER_TAPE {
            let data = generate_random_data(SEGMENT_SIZE);
            store
                .add_mutable_segment(&tape_address, segment_number, data)
                .unwrap();
            store
                .add_mutable_slot(&tape_address, segment_number, segment_number)
                .unwrap();
        }
    }

    let mut group = c.benchmark_group("get_mutable_slot");
    group.bench_function("get_mutable_slot_many_tapes", |b| {
        let tape_address = tape_addresses[NUM_TAPES / 2];
        let segment_number = SEGMENTS_PER_TAPE / 2;

        b.iter(|| {
            store
                .get_mutable_slot(black_box(&tape_address), black_box(segment_number))
                .unwrap();
        })
    });
    group.finish();
}

fn customized_criterion() -> Criterion {
    Criterion::default().sample_size(20)
}

criterion_group! {
    name = benches;
    config = customized_criterion();
    targets = 
        bench_add_mutable_segments,
        bench_add_mutable_slots,
        bench_finalize_tape,
        bench_finalize_many_tapes,
        bench_get_segment,
        bench_get_tape_segments,
        bench_get_slot,
        bench_get_mutable_segment,
        bench_get_mutable_slot
}

criterion_main!(benches);
