//! Opening a segment log flushes its newest file, where a record that was written and never
//! flushed can be, and owes the flush of the older ones. This file sets the process-wide
//! slow-disk pause and failure, so it holds a single test.
#![cfg(feature = "test-support")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_store::segments::{Log, Record, slow_disk};
use std::{fs, io::Write, time::Duration, time::Instant};
use uuid::Uuid;

const PAUSE: Duration = Duration::from_millis(100);

fn record(n: u64) -> Record {
    Record::Data {
        offset: n * 65536,
        bytes: vec![67; 65536],
    }
}

#[test]
fn start_flushes_the_newest_file_and_owes_the_others_and_a_failed_flush_wedges_nothing() {
    let directory = std::env::temp_dir().join(format!("soyeht-segment-open-{}", Uuid::new_v4()));
    let mut log = Log::open(&directory).unwrap();
    // 32 records of 64 KiB fill several segments.
    for n in 0..32 {
        log.append(record(n)).unwrap();
    }
    // The engine ends after writing a record and before its flush.
    log.append_unsynced(record(32)).unwrap();
    drop(log);
    let files = fs::read_dir(&directory).unwrap().count() as u32;
    assert!(files >= 4);
    slow_disk::set(PAUSE);

    // Start waits for one flush: the newest file, so the record is on the disk before anything
    // can acknowledge it (an agent pane has no other flush before its acknowledgement).
    let started = Instant::now();
    let mut log = Log::open(&directory).unwrap();
    let opening = started.elapsed();
    assert!(
        opening >= PAUSE && opening < PAUSE * 2,
        "start waited for {opening:?}"
    );
    assert_eq!(log.next_offset(), 33 * 65536);
    // The older files are owed, to the flush that precedes an acknowledgement.
    let owed = log.take_unsynced().unwrap();
    let flushing = Instant::now();
    owed.flush().unwrap();
    assert!(flushing.elapsed() >= PAUSE * (files - 1), "each older file");
    assert!(
        flushing.elapsed() < PAUSE * files,
        "the newest was not owed"
    );

    // A flush that fails is owed again and leaves the log able to take the next record.
    slow_disk::set(Duration::ZERO);
    log.append_unsynced(record(33)).unwrap();
    let owed = log.take_unsynced().unwrap();
    slow_disk::fail(true);
    assert!(owed.flush().is_err());
    log.restore_unsynced(&owed);
    assert!(
        log.append(record(34)).is_err(),
        "a failing disk fails the append"
    );
    slow_disk::fail(false);
    log.append(record(34)).unwrap();
    assert_eq!(log.next_offset(), 35 * 65536);
    assert!(
        log.take_unsynced().is_none(),
        "the append flushed the file it wrote"
    );

    // A torn last record is cut off, and that file is flushed once before the open returns.
    drop(log);
    let last = fs::read_dir(&directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .max()
        .unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(last)
        .unwrap()
        .write_all(&[0, 1, 2])
        .unwrap();
    slow_disk::set(PAUSE);
    let started = Instant::now();
    let log = Log::open(&directory).unwrap();
    let opening = started.elapsed();
    assert!(log.recovered_truncated_tail);
    assert!(
        opening >= PAUSE && opening < PAUSE * 2,
        "the truncated file waited once: {opening:?}"
    );

    slow_disk::set(Duration::ZERO);
    fs::remove_dir_all(directory).unwrap();
}
