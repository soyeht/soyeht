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
