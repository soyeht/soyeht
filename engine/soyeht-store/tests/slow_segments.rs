//! A segment log written without flushing never waits for the disk, not even when it creates a
//! segment or removes the oldest one; the flush it owes does, once, when it is taken.
//! This file sets the process-wide slow-disk pause, so it holds a single test.
#![cfg(feature = "test-support")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_store::segments::{Log, Record, slow_disk};
use std::{
    fs,
    time::{Duration, Instant},
};
use uuid::Uuid;

const PAUSE: Duration = Duration::from_millis(50);

#[test]
fn rotation_and_retention_owe_their_flush_instead_of_waiting_for_it() {
    let directory = std::env::temp_dir().join(format!("soyeht-slow-segments-{}", Uuid::new_v4()));
    let mut log = Log::open(&directory).unwrap();
    slow_disk::set(PAUSE);

    // 520 records of 64 KiB cross about 65 segments and past the retention limit, so the log
    // creates segments and removes old ones all along. Flushing the file and the directory of
    // each would take over 130 pauses.
    let started = Instant::now();
    for n in 0..520 {
        log.append_unsynced(Record::Data {
            offset: n * 65536,
            bytes: vec![67; 65536],
        })
        .unwrap();
    }
    let writing = started.elapsed();
    assert!(log.first_offset() > 0, "retention removed old segments");
    assert!(
        writing < PAUSE * 60,
        "writing waited {writing:?} for a disk that takes {PAUSE:?} per flush"
    );

    let owed = log.take_unsynced().unwrap();
    let flushing = Instant::now();
    owed.flush().unwrap();
    assert!(
        flushing.elapsed() >= PAUSE,
        "the owed flush waits for the disk"
    );
    assert!(log.take_unsynced().is_none());

    // A flush that failed is owed again, directory included.
    let tail = 520 * 65536;
    let record = Record::Data {
        offset: tail,
        bytes: b"tail".to_vec(),
    };
    log.append_unsynced(record).unwrap();
    let failed = log.take_unsynced().unwrap();
    log.restore_unsynced(&failed);
    assert!(log.take_unsynced().is_some());

    slow_disk::set(Duration::ZERO);
    fs::remove_dir_all(directory).unwrap();
}
