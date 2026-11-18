//! The processor time the calling thread has used, which a host that is busy with other work does
//! not inflate the way the clock on the wall is: a program that paces its own work by what it
//! costs reads this.

use std::time::Duration;

/// The processor time (user and system) the calling thread has used since it began, or `None`
/// where the system does not say.
pub fn thread_time() -> Option<Duration> {
    system()
}

#[cfg(unix)]
fn system() -> Option<Duration> {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid, writable `timespec` for the whole call, which only fills it in.
    let done = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
    if done != 0 {
        return None;
    }
    let seconds = u64::try_from(now.tv_sec).ok()?;
    let nanos = u32::try_from(now.tv_nsec).ok()?;
    Some(Duration::new(seconds, nanos))
}

#[cfg(windows)]
fn system() -> Option<Duration> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentThread, GetThreadTimes},
    };
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: the handle is the pseudo-handle of this thread, which is always valid, and the four
    // pointers are valid, writable `FILETIME`s for the whole call.
    let done = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    if done == 0 {
        return None;
    }
    // Both are counts of 100 nanoseconds.
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    let total = ticks(kernel).checked_add(ticks(user))?;
    Some(Duration::from_nanos(total.checked_mul(100)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_of_a_thread_grows_with_what_it_does() {
        let Some(before) = thread_time() else {
            return;
        };
        let mut sum = 0u64;
        for i in 0..20_000_000u64 {
            sum = sum.wrapping_add(std::hint::black_box(i) ^ (sum >> 3));
        }
        std::hint::black_box(sum);
        let after = thread_time().unwrap_or(before);
        assert!(after > before);
    }
}
