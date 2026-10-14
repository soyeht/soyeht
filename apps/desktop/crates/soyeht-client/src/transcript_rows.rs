//! Which rows of a long transcript a frame builds.
//!
//! A frame lays out every element it is given. A transcript row costs about a tenth of a
//! millisecond to lay out, a pane holds up to `agent::TAIL` of them, and every repaint (a key in
//! the composer, a hover, a streamed chunk) lays all of them out again: about 25 ms for the 100
//! rows of a long conversation, three frames of a 60 Hz display. Only a few rows are on screen.
//!
//! So only the rows near the viewport are built. Each of the others is a spacer of the height the
//! row had the last time it was built, so the scroll range does not change. A row is built
//! whenever its height cannot be trusted: it was never measured, its content or its open state
//! changed, or the width it was measured at is no longer the width of the transcript.

use std::{
    collections::HashMap,
    fmt::{self, Debug, Write},
    hash::{DefaultHasher, Hasher},
};

/// Viewports of rows built above and below the one on screen.
const MARGIN: f32 = 0.5;
/// The newest rows are always built: a reply that is still growing lays out in full.
const NEWEST: usize = 8;
/// A transcript this short is built whole: laying it out costs less than keeping track of it.
const SHORT: usize = 40;
/// What a row that was never measured is assumed to be, to place the rows around it.
const GUESS: f32 = 120.0;
/// Widths closer than this are the same width.
const SAME_WIDTH: f32 = 0.5;

/// Where the transcript is scrolled to, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// The height of the scrolling area; zero before it has been laid out once.
    pub height: f32,
    /// How far the content continues below the viewport; zero when the transcript follows the
    /// newest message.
    pub from_bottom: f32,
}

/// What one child of the scrolling column was in the frame that built it.
#[derive(Debug, Clone, PartialEq)]
pub enum Child {
    /// Anything that is not a transcript row: the earlier-messages button, a notice.
    Other,
    Row {
        key: String,
        fingerprint: u64,
        /// The row itself was built, so its laid-out height is its own and can be kept.
        built: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct Measure {
    fingerprint: u64,
    width: f32,
    /// `None` when the entry draws no row at all, so it takes no room and no gap.
    height: Option<f32>,
}

/// The heights the rows of one pane's transcript were laid out at, and the plan of the frame.
#[derive(Debug, Default)]
pub struct Rows {
    measured: HashMap<String, Measure>,
    children: Vec<Child>,
    /// The width of the rows in the last frame that was laid out.
    width: f32,
    /// What the window looked like when the last frame was planned.
    layout: u64,
}

impl Rows {
    /// Decides, for each row of `keys` (the key of the row and the fingerprint of its content),
    /// whether this frame builds it (`None`) or stands in a spacer of that height for it.
    /// `gap` is the space between rows; `layout` stands for everything that decides the width of
    /// the transcript besides its own size (the window, the panes beside it): when it changes the
    /// widths measured so far are not those of this frame, so nothing is left out.
    pub fn plan(
        &mut self,
        keys: &[(String, u64)],
        viewport: Viewport,
        gap: f32,
        layout: u64,
    ) -> Vec<Option<f32>> {
        let settled = layout == self.layout;
        self.layout = layout;
        self.measured
            .retain(|key, _| keys.iter().any(|(held, _)| held == key));
        let known: Vec<Option<Option<f32>>> = keys
            .iter()
            .map(|(key, fingerprint)| self.height_of(key, *fingerprint))
            .collect();
        let mut plan = vec![None; keys.len()];
        if keys.len() <= SHORT || viewport.height <= 0.0 || !settled {
            return plan;
        }
        let margin = viewport.height * MARGIN;
        let (window_top, window_bottom) = (
            viewport.from_bottom - margin,
            viewport.from_bottom + viewport.height + margin,
        );
        // Distance of each row's lower edge from the end of the content, nearest row first.
        let mut below = 0.0_f32;
        for (at, height) in known.iter().enumerate().rev() {
            // An entry that draws nothing is built again (it costs nothing) and takes no room.
            if *height == Some(None) {
                continue;
            }
            let row = height.flatten().unwrap_or(GUESS);
            let (near, far) = (below, below + row);
            below = far + gap;
            let newest = keys.len() - at <= NEWEST;
            let on_screen = far >= window_top && near <= window_bottom;
            if let Some(Some(height)) = height
                && !newest
                && !on_screen
            {
                plan[at] = Some(*height);
            }
        }
        plan
    }

    /// Records what the frame just planned put in the column, in order.
    pub fn set_children(&mut self, children: Vec<Child>) {
        self.children = children;
    }

    /// Reads the sizes the frame laid the column's children out at, in order.
    pub fn absorb(&mut self, sizes: &[(f32, f32)]) {
        if sizes.is_empty() {
            return;
        }
        // The rows are as wide as the column; a button or a notice beside them may be narrower.
        let width = sizes
            .iter()
            .fold(0.0_f32, |widest, size| widest.max(size.0));
        self.width = width;
        for (child, (_, height)) in self.children.iter().zip(sizes) {
            if let Child::Row {
                key,
                fingerprint,
                built: true,
            } = child
            {
                self.measured.insert(
                    key.clone(),
                    Measure {
                        fingerprint: *fingerprint,
                        width,
                        height: Some(*height),
                    },
                );
            }
        }
    }

    /// Notes that the entry drew no row, so that it is not taken for one that was never measured.
    pub fn mark_absent(&mut self, key: &str, fingerprint: u64) {
        let measure = Measure {
            fingerprint,
            width: self.width,
            height: None,
        };
        self.measured.insert(key.to_owned(), measure);
    }

    fn height_of(&self, key: &str, fingerprint: u64) -> Option<Option<f32>> {
        let measure = self.measured.get(key)?;
        (measure.fingerprint == fingerprint && (measure.width - self.width).abs() < SAME_WIDTH)
            .then_some(measure.height)
    }
}

/// A number that changes whenever `value` (or `state`, what the row shows that `value` does not
/// hold, such as whether a thought is open) changes in a way that can change the row's height.
pub fn fingerprint(value: &dyn Debug, state: u64) -> u64 {
    struct Feed(DefaultHasher);
    impl Write for Feed {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.0.write(text.as_bytes());
            Ok(())
        }
    }
    let mut feed = Feed(DefaultHasher::new());
    feed.0.write_u64(state);
    // The writer never fails, so a failure here is the value's own `Debug` giving up.
    if write!(feed, "{value:?}").is_err() {
        feed.0.write_u8(1);
    }
    feed.0.finish()
}

#[cfg(test)]
mod tests;
