//! Which rows of a long transcript a frame builds.
//!
//! A frame lays out every element it is given. A transcript row costs about a tenth of a
//! millisecond to lay out, a pane holds up to `agent::TAIL` of them, and every repaint (a key in
//! the composer, a hover, a streamed chunk) lays all of them out again: about 25 ms for the 100
//! rows of a long conversation, three frames of a 60 Hz display. Only a few rows are on screen.
//! A row of a long reply costs hundreds of times that, and on a machine that draws in software
//! the layout of one frame of such a transcript took longer than the next repaint was due.
//!
//! So only the rows near the viewport are built. Each of the others is a spacer of the height the
//! row had the last time it was built, so the scroll range does not change. A row is built
//! whenever its height cannot be trusted: its content or its open state changed, or the width it
//! was measured at is no longer the width of the transcript. A row that was never measured and is
//! far from the viewport is not built at once either: each frame measures the nearest of them,
//! as far as its [`FRAME_BUDGET`] goes, and stands a guess in for the rest, until the repaints it
//! asks for ([`Rows::catching_up`]) have measured them all. A frame therefore costs about the
//! rows on screen and one budget, however long the transcript or its replies. While rows are
//! being measured, a transcript pinned to its newest message is pinned again each frame; one the
//! person scrolled up in is not moved, so what is above the viewport may shift by the difference
//! between a guess and the real height of a row measured for the first time.

use crate::{agent::AgentView, cards::Entry};
use std::{
    collections::HashMap,
    fmt::{self, Debug, Write},
    hash::{DefaultHasher, Hasher},
};

/// Viewports of rows built above and below the one on screen.
const MARGIN: f32 = 0.5;
/// The newest rows are always built: a reply that is still growing lays out in full.
const NEWEST: usize = 8;
/// What a frame builds besides the rows on screen and the newest, in the weight of the rows (the unit of
/// `markdown::weight_of`: about one list item): the nearest of the rows never measured. A
/// transcript that weighs no more than this is built whole: laying it out costs less than keeping
/// track of it.
pub const FRAME_BUDGET: usize = 200;
/// How tall the viewport is taken to be before its first layout.
const ASSUMED_VIEWPORT: f32 = 1000.0;
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

/// A row of the transcript, as a frame plans it.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub key: String,
    /// Changes whenever the content of the row changes ([`entry_fingerprint`]).
    pub fingerprint: u64,
    /// What building the row costs, in the weight of `markdown::weight_of`.
    pub weight: usize,
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
    /// What the window looked like when the last frame was planned; none before the first.
    layout: Option<u64>,
    /// The last frame left a row as a guess: it asks to be drawn again.
    catching_up: bool,
    /// A row before the newest ones was built without a height that holds.
    measuring: bool,
}

impl Rows {
    /// Decides, for each row of `keys`, whether this frame builds it (`None`) or stands in a
    /// spacer of that height for it. `gap` is the space between rows; `layout` stands for
    /// everything that decides the width of the transcript besides its own size (the window, the
    /// panes beside it): when it changes the widths measured so far are not those of this frame,
    /// so none of them is trusted.
    pub fn plan(
        &mut self,
        keys: &[Key],
        viewport: Viewport,
        gap: f32,
        layout: u64,
    ) -> Vec<Option<f32>> {
        let settled = self.layout.is_none_or(|last| last == layout);
        self.layout = Some(layout);
        self.catching_up = false;
        self.measured
            .retain(|key, _| keys.iter().any(|held| &held.key == key));
        // When the window changed, the widths measured so far are not the width of this frame,
        // whatever `absorb` last saw: no height holds.
        let known: Vec<Option<Option<f32>>> = keys
            .iter()
            .map(|row| self.height_of(&row.key, row.fingerprint))
            .map(|height| height.filter(|height| settled || height.is_none()))
            .collect();
        let mut plan = vec![None; keys.len()];
        // The newest rows change height as they stream; the others change the height above the
        // newest when they are measured.
        self.measuring = known[..keys.len().saturating_sub(NEWEST)]
            .iter()
            .any(Option::is_none);
        let weight: usize = keys.iter().map(|row| row.weight).sum();
        if weight <= FRAME_BUDGET {
            return plan;
        }
        let height = if viewport.height > 0.0 {
            viewport.height
        } else {
            ASSUMED_VIEWPORT
        };
        let margin = height * MARGIN;
        let (window_top, window_bottom) = (
            viewport.from_bottom - margin,
            viewport.from_bottom + height + margin,
        );
        // Distance of each row's lower edge from the end of the content, nearest row first, and
        // what the never-measured rows built so far, off screen and not among the newest, weigh.
        let mut below = 0.0_f32;
        let mut far_spent = 0;
        for (at, height) in known.iter().enumerate().rev() {
            // An entry that draws nothing is built again (it costs nothing) and takes no room.
            if *height == Some(None) {
                continue;
            }
            // A row that is not trusted still has about the height it had, for placing the others.
            let last = self.measured.get(&keys[at].key).and_then(|m| m.height);
            let row = height.flatten().or(last).unwrap_or(GUESS);
            let (near, far) = (below, below + row);
            below = far + gap;
            let newest = keys.len() - at <= NEWEST;
            let on_screen = far >= window_top && near <= window_bottom;
            let cost = keys[at].weight;
            if newest || on_screen {
                // Built whatever it weighs, and not charged to the budget: what is on screen must
                // not starve the measuring of the rest.
            } else if let Some(Some(height)) = height {
                plan[at] = Some(*height);
            } else if far_spent == 0 || far_spent + cost <= FRAME_BUDGET {
                // The first one of a frame is admitted over the budget: a row heavier than the
                // budget would otherwise never be measured.
                far_spent += cost;
            } else {
                plan[at] = Some(last.unwrap_or(GUESS));
                self.catching_up = true;
            }
        }
        plan
    }

    /// Whether the last frame left rows as guesses, so that the window should be drawn again to
    /// measure the next of them.
    pub fn catching_up(&self) -> bool {
        self.catching_up
    }

    /// Whether the last frame built a row it had not measured before the newest ones, so that the
    /// height above the newest changed: a transcript pinned to its newest message is pinned again.
    pub fn measuring(&self) -> bool {
        self.measuring
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

/// A number that changes whenever `entry` or `state` (what the row shows that the entry does not
/// hold, such as whether a thought is open) changes. It reads the revisions the view keeps, not
/// the content, so a frame pays a few lookups per entry however long the transcript is. A
/// subagent's row stands for its own message and for every entry under it, in order.
pub fn entry_fingerprint(entry: &Entry<'_>, view: &AgentView, state: u64) -> u64 {
    fn feed(entry: &Entry<'_>, view: &AgentView, hasher: &mut DefaultHasher) {
        match entry {
            Entry::Message(message) => hasher.write_u64(view.revision_of(&message.local_id)),
            Entry::Subagent(group) => {
                hasher.write_u64(group.row.map_or(0, |row| view.revision_of(row)));
                hasher.write_usize(group.entries.len());
                for inner in &group.entries {
                    feed(inner, view, hasher);
                }
            }
        }
    }
    let mut hasher = DefaultHasher::new();
    hasher.write_u64(state);
    feed(entry, view, &mut hasher);
    hasher.finish()
}

/// A number that changes whenever `value` (or `state`) changes, for a small value such as the
/// layout of the window. A transcript entry uses [`entry_fingerprint`]: formatting a message
/// costs as much as its text is long.
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
