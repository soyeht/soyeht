//! When a screen is fitted to the room its view has. A size that keeps changing (a divider or a
//! window being dragged) is not fitted at every frame: that would cut the rows again for each
//! one, and tell the shell of each size when what it needs is the last. The screen is fitted when
//! the size has held for a moment, and at once the first time, when it has nothing to lose.

use std::time::{Duration, Instant};

/// How long a size must hold before the screen is fitted to it.
pub const SETTLE: Duration = Duration::from_millis(120);

/// What to do with a size the view asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// The screen already has it.
    Nothing,
    /// Fit the screen to it now.
    Now,
    /// Not yet. `ask_again` is set when this is a new size, whose settling needs a frame once
    /// `SETTLE` has gone by: the caller arranges one.
    Wait { ask_again: bool },
}

/// The sizes one screen's view has asked for.
#[derive(Debug, Default)]
pub struct Settle {
    fitted: bool,
    asked: Option<((u16, u16), Instant)>,
}

impl Settle {
    /// The view asks for `wanted` `(rows, columns)` at `now`; the screen has `current`.
    pub fn offer(&mut self, wanted: (u16, u16), current: (u16, u16), now: Instant) -> Fit {
        if wanted == current {
            self.fitted = true;
            self.asked = None;
            return Fit::Nothing;
        }
        if !self.fitted {
            self.fitted = true;
            self.asked = None;
            return Fit::Now;
        }
        match self.asked {
            Some((size, since)) if size == wanted => {
                if now.duration_since(since) >= SETTLE {
                    self.asked = None;
                    Fit::Now
                } else {
                    Fit::Wait { ask_again: false }
                }
            }
            _ => {
                self.asked = Some((wanted, now));
                Fit::Wait { ask_again: true }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_size_is_fitted_at_once() {
        let mut settle = Settle::default();
        let now = Instant::now();
        assert_eq!(settle.offer((24, 80), (24, 80), now), Fit::Nothing);
        let mut fresh = Settle::default();
        assert_eq!(fresh.offer((30, 100), (24, 80), now), Fit::Now);
    }

    #[test]
    fn a_size_that_keeps_changing_is_not_fitted_until_it_holds() {
        let mut settle = Settle::default();
        let start = Instant::now();
        assert_eq!(settle.offer((30, 100), (24, 80), start), Fit::Now);
        let at = |ms| start + Duration::from_millis(ms);
        // A drag: a new size at every frame. None of them is fitted, and only the first of
        // each asks for a frame.
        let mut asks = 0;
        for (n, cols) in (101..140).enumerate() {
            let step = settle.offer((30, cols), (30, 100), at(16 * n as u64 + 1));
            assert!(matches!(step, Fit::Wait { .. }), "{cols}: {step:?}");
            asks += usize::from(step == Fit::Wait { ask_again: true });
        }
        assert_eq!(asks, 39);
        // The drag stops at 139 columns: the same size again does not ask twice, and is fitted
        // once it has held.
        let last = 16 * 38 + 1;
        assert_eq!(
            settle.offer((30, 139), (30, 100), at(last + 50)),
            Fit::Wait { ask_again: false }
        );
        assert_eq!(settle.offer((30, 139), (30, 100), at(last + 130)), Fit::Now);
        assert_eq!(
            settle.offer((30, 139), (30, 139), at(last + 140)),
            Fit::Nothing
        );
    }

    #[test]
    fn going_back_to_the_size_the_screen_has_forgets_what_was_asked() {
        let mut settle = Settle::default();
        let start = Instant::now();
        settle.offer((24, 80), (24, 80), start);
        assert!(matches!(
            settle.offer((24, 40), (24, 80), start),
            Fit::Wait { .. }
        ));
        assert_eq!(settle.offer((24, 80), (24, 80), start), Fit::Nothing);
        assert_eq!(
            settle.offer((24, 40), (24, 80), start + SETTLE * 2),
            Fit::Wait { ask_again: true }
        );
    }
}
