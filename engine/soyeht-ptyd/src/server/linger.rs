//! The end of a connection. A peer that is still sending when the supervisor ends the connection
//! (a request that does not decode, a second `hello`, a refusal) leaves unread bytes behind, and
//! closing a socket that holds unread bytes resets the connection on Windows' loopback TCP: the
//! peer then loses what the supervisor wrote but it had not read yet, the refusal included, or
//! keeps half of a frame. The supervisor says it is done writing and reads what the peer still
//! sends, up to a limit, before it closes.

use soyeht_platform::local::Stream;
use std::io::Read;
use std::net::Shutdown;
use std::time::{Duration, Instant};

/// The longest the supervisor waits for a peer to stop sending once it has ended a connection.
const DRAIN_DEADLINE: Duration = Duration::from_secs(1);
/// The most it reads from a peer that does not stop.
const DRAIN_LIMIT: usize = 64 * 1024;

/// Serves `stream` with `serve`, then closes it as described above, whatever `serve` returned.
pub(super) fn after<T>(mut stream: Stream, serve: impl FnOnce(&mut Stream) -> T) -> T {
    let result = serve(&mut stream);
    finish(&mut stream);
    result
}

fn finish(stream: &mut Stream) {
    // A peer that already reset the connection leaves nothing to drain, and nothing to report:
    // the connection ends either way.
    if stream.shutdown(Shutdown::Write).is_err() {
        return;
    }
    let started = Instant::now();
    let mut spent = 0;
    let mut buffer = [0_u8; 1024];
    while spent < DRAIN_LIMIT {
        let Some(left) = DRAIN_DEADLINE.checked_sub(started.elapsed()) else {
            return;
        };
        if stream.set_read_timeout(Some(left)).is_err() {
            return;
        }
        // The end of the peer's input, a reset and the deadline all end the drain.
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        spent += read;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_platform::local::pair;
    use std::io::Write;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::thread;

    /// How long ending a connection takes, and the peer if it is still there. `peer` acts on its
    /// end first and says whether it keeps it.
    fn ended(
        peer: impl FnOnce(&mut Stream) -> bool,
        serve: impl FnOnce(&mut Stream),
    ) -> (Duration, Option<Stream>) {
        let (mut theirs, ours) = pair().unwrap();
        // Set now: some systems refuse a timeout on a socket whose peer has closed.
        theirs
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let theirs = peer(&mut theirs).then_some(theirs);
        let started = Instant::now();
        after(ours, serve);
        (started.elapsed(), theirs)
    }

    /// Whether the peer is told that the connection is over: it reads the end of the stream.
    fn sees_the_end(peer: &mut Stream) -> bool {
        let mut byte = [0_u8; 1];
        matches!(peer.read(&mut byte), Ok(0))
    }

    #[test]
    fn a_peer_that_closed_leaves_nothing_to_wait_for() {
        let (took, _) = ended(|_| false, |_| {});
        assert!(took < Duration::from_millis(500), "{took:?}");
    }

    #[test]
    fn a_peer_that_ended_its_input_is_not_waited_for_and_sees_the_end() {
        let (took, peer) = ended(
            |peer| {
                peer.shutdown(Shutdown::Write).unwrap();
                true
            },
            |_| {},
        );
        assert!(took < Duration::from_millis(500), "{took:?}");
        assert!(sees_the_end(&mut peer.unwrap()));
    }

    #[test]
    fn a_connection_a_clone_already_shut_down_in_both_directions_ends_at_once() {
        let (took, _) = ended(
            |peer| {
                peer.write_all(b"unread").unwrap();
                true
            },
            |stream| {
                stream
                    .try_clone()
                    .unwrap()
                    .shutdown(Shutdown::Both)
                    .unwrap()
            },
        );
        assert!(took < Duration::from_millis(500), "{took:?}");
    }

    #[test]
    fn what_the_peer_sends_after_the_end_is_read_and_the_peer_is_told_it_is_over() {
        // The peer keeps its input open and writes after the supervisor is done writing.
        let (mut theirs, ours) = pair().unwrap();
        theirs
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let ending = thread::spawn(move || after(ours, |_| {}));
        thread::sleep(Duration::from_millis(100));
        theirs.write_all(&[7; 20_000]).unwrap();
        theirs.shutdown(Shutdown::Write).unwrap();
        ending.join().unwrap();
        assert!(sees_the_end(&mut theirs));
    }

    #[test]
    fn a_peer_that_floods_is_cut_at_the_limit_and_not_waited_for_past_the_deadline() {
        let (mut theirs, ours) = pair().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flooding = {
            let (stop, mut flood) = (Arc::clone(&stop), theirs.try_clone().unwrap());
            thread::spawn(move || {
                let chunk = [1_u8; 8192];
                while !stop.load(Ordering::Relaxed) && flood.write_all(&chunk).is_ok() {}
            })
        };
        let started = Instant::now();
        after(ours, |_| {});
        let took = started.elapsed();
        assert!(
            took < DRAIN_DEADLINE + Duration::from_millis(500),
            "{took:?}"
        );
        // The supervisor has closed: the flood ends on its own, with an error.
        flooding.join().unwrap();
        stop.store(true, Ordering::Relaxed);
        assert!(theirs.write_all(&[1; 8192]).is_err() || sees_the_end(&mut theirs));
    }

    #[test]
    fn a_peer_that_stays_silent_is_waited_for_about_the_deadline() {
        let (took, _) = ended(|_| true, |_| {});
        assert!(
            took >= DRAIN_DEADLINE - Duration::from_millis(100),
            "{took:?}"
        );
        assert!(
            took < DRAIN_DEADLINE + Duration::from_millis(500),
            "{took:?}"
        );
    }

    #[test]
    fn a_peer_that_drips_one_byte_at_a_time_does_not_stretch_the_deadline() {
        let (mut theirs, ours) = pair().unwrap();
        let dripping = thread::spawn(move || {
            while theirs.write_all(&[1]).is_ok() {
                thread::sleep(Duration::from_millis(50));
            }
        });
        let started = Instant::now();
        after(ours, |_| {});
        let took = started.elapsed();
        assert!(
            took < DRAIN_DEADLINE + Duration::from_millis(500),
            "{took:?}"
        );
        dripping.join().unwrap();
    }

    #[test]
    fn the_result_of_serving_is_returned() {
        let (_theirs, ours) = pair().unwrap();
        assert_eq!(after(ours, |_| 42), 42);
    }
}
