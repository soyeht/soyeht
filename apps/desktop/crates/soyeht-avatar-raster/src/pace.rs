//! How often the avatar is drawn and at what size, from what drawing it has cost.
//!
//! A picture is drawn 30 times a second, and 60 while the avatar speaks (the mouth is what is
//! looked at then). The size is what the view shows, no more than `WIDEST` (the largest picture
//! until the cost of a frame on Windows is measured), and no more than the processor time of the
//! drawing thread allows: a frame that took more than three fifths of the time it had makes the
//! next pictures a quarter narrower, and when the frames have taken less than a third of it for
//! three seconds the picture grows back by a third. The cost is the thread's processor time
//! (`soyeht_platform::cpu`), which a host busy with other work does not inflate the way the
//! clock does; where the system does not say it, the clock is used.

use std::time::Duration;

/// A picture every this often when the avatar is not speaking: 30 a second.
pub const IDLE_INTERVAL: Duration = Duration::from_micros(33_333);
/// A picture every this often while it speaks: 60 a second.
pub const SPEAKING_INTERVAL: Duration = Duration::from_micros(16_667);
/// The widest picture drawn, in pixels across, until a frame is measured on Windows.
pub const WIDEST: u32 = 2 * 256;
/// The narrowest: a view the size of a thumbnail is not drawn smaller.
pub const NARROWEST: u32 = 64;
/// A frame that cost more than this share of its interval is an overrun.
const OVERRUN: f64 = 0.6;
/// Frames that cost less than this share of their interval are quiet.
const QUIET: f64 = 0.3;
/// How long the frames stay quiet before the picture grows back.
const GROW_AFTER: Duration = Duration::from_secs(3);

/// What to draw next: the time to the frame after it, and the width of the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    pub interval: Duration,
    pub width: u32,
}

/// The largest width the cost of the frames has left, and how long they have been quiet.
#[derive(Debug, Clone, Copy)]
pub struct Pacer {
    cap: u32,
    calm: Duration,
}

impl Default for Pacer {
    fn default() -> Self {
        Self {
            cap: WIDEST,
            calm: Duration::ZERO,
        }
    }
}

impl Pacer {
    /// The pace for a view that shows `wanted` pixels across.
    pub fn next(&self, wanted: u32, speaking: bool) -> Pace {
        Pace {
            interval: if speaking {
                SPEAKING_INTERVAL
            } else {
                IDLE_INTERVAL
            },
            width: wanted.clamp(NARROWEST, self.cap.max(NARROWEST)),
        }
    }

    /// Takes in what a frame drawn at `pace` cost the drawing thread.
    pub fn record(&mut self, pace: Pace, cost: Duration) {
        let share = cost.as_secs_f64() / pace.interval.as_secs_f64();
        if share > OVERRUN {
            self.cap = (pace.width * 3 / 4).max(NARROWEST);
            self.calm = Duration::ZERO;
        } else if share < QUIET {
            self.calm += pace.interval;
            if self.calm >= GROW_AFTER {
                self.cap = (self.cap * 4 / 3).min(WIDEST);
                self.calm = Duration::ZERO;
            }
        } else {
            self.calm = Duration::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOW: Duration = Duration::from_millis(30);
    const FAST: Duration = Duration::from_millis(2);

    #[test]
    fn it_draws_thirty_a_second_and_sixty_while_speaking() {
        let pacer = Pacer::default();
        assert_eq!(pacer.next(300, false).interval, IDLE_INTERVAL);
        assert_eq!(pacer.next(300, true).interval, SPEAKING_INTERVAL);
    }

    #[test]
    fn the_picture_is_what_the_view_shows_within_the_limits() {
        let pacer = Pacer::default();
        assert_eq!(pacer.next(300, false).width, 300);
        assert_eq!(pacer.next(4000, false).width, WIDEST);
        assert_eq!(pacer.next(10, false).width, NARROWEST);
    }

    #[test]
    fn a_frame_that_overran_makes_the_next_pictures_narrower_down_to_a_floor() {
        let mut pacer = Pacer::default();
        let mut width = WIDEST;
        for _ in 0..20 {
            let pace = pacer.next(WIDEST, true);
            assert!(pace.width <= width);
            width = pace.width;
            pacer.record(pace, SLOW);
        }
        assert_eq!(width, NARROWEST);
        assert_eq!(pacer.next(WIDEST, true).width, NARROWEST);
    }

    #[test]
    fn it_grows_back_after_a_quiet_period_and_not_before() {
        let mut pacer = Pacer::default();
        pacer.record(pacer.next(WIDEST, false), SLOW);
        let narrow = pacer.next(WIDEST, false).width;
        assert!(narrow < WIDEST);
        // Two seconds of quiet frames at 30 a second: still as narrow.
        for _ in 0..60 {
            pacer.record(pacer.next(WIDEST, false), FAST);
        }
        assert_eq!(pacer.next(WIDEST, false).width, narrow);
        // Past three seconds it grows, by a third each quiet period, never past the widest.
        for _ in 0..40 {
            pacer.record(pacer.next(WIDEST, false), FAST);
        }
        assert!(pacer.next(WIDEST, false).width > narrow);
        for _ in 0..2000 {
            pacer.record(pacer.next(WIDEST, false), FAST);
        }
        assert_eq!(pacer.next(WIDEST, false).width, WIDEST);
    }

    #[test]
    fn a_frame_in_between_restarts_the_quiet_period() {
        let mut pacer = Pacer::default();
        pacer.record(pacer.next(WIDEST, false), SLOW);
        let narrow = pacer.next(WIDEST, false).width;
        for round in 0..3 {
            for _ in 0..80 {
                pacer.record(pacer.next(WIDEST, false), FAST);
            }
            // Half the interval: neither quiet nor an overrun.
            pacer.record(pacer.next(WIDEST, false), Duration::from_millis(15));
            assert_eq!(pacer.next(WIDEST, false).width, narrow, "round {round}");
        }
    }
}
