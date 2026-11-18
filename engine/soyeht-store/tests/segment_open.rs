//! Opening a segment log does not flush the files it only read: it owes their flush, which
//! comes before the pane's output is acknowledged. It flushes at once only a file it truncated.
//! This file sets the process-wide slow-disk pause, so it holds a single test.
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
fn start_owes_the_flush_of_the_files_it_read_and_makes_only_a_truncation_wait() {
    let directory = std::env::temp_dir().join(format!("soyeht-segment-open-{}", Uuid::new_v4()));
    let mut log = Log::open(&directory).unwrap();
    // 32 records of 64 KiB fill four segments of 512 KiB.
    for n in 0..32 {
        log.append(record(n)).unwrap();
    }
    drop(log);
    slow_disk::set(PAUSE);

    let started = Instant::now();
    let mut log = Log::open(&directory).unwrap();
    assert!(started.elapsed() < PAUSE, "start waited for a flush");
    assert_eq!(log.next_offset(), 32 * 65536);
    let flushing = Instant::now();
    log.take_unsynced().unwrap().flush().unwrap();
    assert!(
        flushing.elapsed() >= PAUSE * 4,
        "each segment read at start is owed"
    );

    // A flushed append also flushes what is owed from the start.
    drop(log);
    let mut log = Log::open(&directory).unwrap();
    let appending = Instant::now();
    log.append(record(32)).unwrap();
    assert!(
        appending.elapsed() >= PAUSE * 4,
        "append flushed the owed files"
    );
    assert!(log.take_unsynced().is_none());

    // A torn last record is cut off, and that file is flushed before the open returns.
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
    let started = Instant::now();
    let log = Log::open(&directory).unwrap();
    let opening = started.elapsed();
    assert!(log.recovered_truncated_tail);
    assert!(
        opening >= PAUSE && opening < PAUSE * 2,
        "only the truncated file waited: {opening:?}"
    );

    slow_disk::set(Duration::ZERO);
    fs::remove_dir_all(directory).unwrap();
}
