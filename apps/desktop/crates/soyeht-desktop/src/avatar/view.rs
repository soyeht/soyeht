//! The avatar's stage: a view that shows one avatar and keeps it alive.
//!
//! Each frame the stage asks its `Driver` what the avatar is doing (the conversation's state and
//! the audio that plays now), steps the runtime by the time that really passed, and has the
//! picture drawn **on the background executor**; the finished picture is assigned, and painted, on
//! the main thread. The pace is that of `Pacer`: 30 pictures a second, 60 while the avatar
//! speaks, and a picture that is narrower for a while after a frame that cost too much of the
//! drawing thread's processor time. The stage draws only while it is painted: a window that is
//! hidden or minimised stops asking for frames, and the timer that wakes the stage ends with it.

use crate::{
    picture::{paint_picture, painted, swap_picture},
    theme::Palette,
};
use gpui::{
    Bounds, Context, IntoElement, ParentElement, Pixels, Render, RenderImage, Styled, Task, Window,
    div, point, px, rgb, size,
};
use soyeht_avatar::{Audio, Frame, Runtime, State, Tune};
use soyeht_avatar_raster::{Avatar, Pace, Pacer, Rasteriser};
use soyeht_platform::cpu::thread_time;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// A step is never longer than this, however long the window was away (seconds): the acting
/// layer is not told that a minute went by in one step.
const LONGEST_STEP: f64 = 0.1;
/// The first step, before there is a frame before it (seconds).
const FIRST_STEP: f64 = 1.0 / 60.0;
/// A stage that has not been painted for this long is hidden: its timer stops.
const HIDDEN_AFTER: Duration = Duration::from_millis(500);

/// What the avatar is doing now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drive {
    /// The state the conversation is in.
    pub state: State,
    /// The audio that plays, with its place in the clip and its level; `None` when it is silent.
    pub audio: Option<Audio>,
}

/// What animates the avatar: asked once a frame, with the seconds since the frame before.
pub trait Driver: 'static {
    fn drive(&mut self, step: f64) -> Drive;
}

/// An avatar that does nothing but live: idle, with no audio.
pub struct Resting;

impl Driver for Resting {
    fn drive(&mut self, _step: f64) -> Drive {
        Drive {
            state: State::Idle,
            audio: None,
        }
    }
}

/// A picture the drawing thread has finished: BGRA bytes and what it cost.
struct Finished {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    pace: Pace,
    /// The processor time of the drawing thread, or the time on the clock where the system does
    /// not give it.
    cost: Duration,
}

pub struct AvatarStage {
    palette: &'static Palette,
    avatar: Arc<Avatar>,
    /// The rasteriser, which the drawing thread has while a picture is being made.
    raster: Option<Rasteriser>,
    runtime: Runtime,
    driver: Box<dyn Driver>,
    pacer: Pacer,
    /// When the last step of the runtime was.
    last_step: Option<Instant>,
    /// When the next picture is to be started.
    due: Instant,
    /// When the stage was last painted.
    painted: Option<Instant>,
    /// A picture drawn and not yet on screen.
    finished: Option<Finished>,
    /// The picture on screen.
    picture: Option<Arc<RenderImage>>,
    /// The drawing in progress, and the timer to the next one.
    _drawing: Option<Task<()>>,
    _wake: Option<Task<()>>,
}

impl AvatarStage {
    /// A stage for `avatar`, animated by `driver`. The same `seed` gives the same blinks and
    /// glances.
    pub fn new(
        avatar: Arc<Avatar>,
        driver: impl Driver,
        seed: u64,
        palette: &'static Palette,
    ) -> Self {
        Self {
            palette,
            runtime: Runtime::new(avatar.package(), Tune::default(), seed),
            raster: Some(Rasteriser::new(Arc::clone(&avatar))),
            avatar,
            driver: Box::new(driver),
            pacer: Pacer::default(),
            last_step: None,
            due: Instant::now(),
            painted: None,
            finished: None,
            picture: None,
            _drawing: None,
            _wake: None,
        }
    }

    /// Steps the runtime by the time since the last step and asks the driver what it does.
    fn next_frame(&mut self, now: Instant) -> (Frame, bool) {
        let step = self.last_step.replace(now).map_or(FIRST_STEP, |last| {
            now.duration_since(last)
                .as_secs_f64()
                .clamp(0.0, LONGEST_STEP)
        });
        let Drive { state, audio } = self.driver.drive(step);
        let speaking = state == State::Speaking || audio.is_some();
        (self.runtime.step(step, state, audio), speaking)
    }

    /// Starts the next picture if it is due and none is being drawn, or sets a timer for when it
    /// is due.
    fn keep_going(&mut self, shown: Bounds<Pixels>, scale: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self.raster.is_none() {
            // A picture is being drawn: its end wakes the stage.
            return;
        }
        if now < self.due {
            self.wake_at(self.due - now, cx);
            return;
        }
        let (frame, speaking) = self.next_frame(now);
        let across = (f32::from(shown.size.width) * scale).ceil() as u32;
        let pace = self.pacer.next(across, speaking);
        self.due = now + pace.interval;
        let Some(mut raster) = self.raster.take() else {
            return;
        };
        let drawing = cx.background_executor().spawn(async move {
            let (cpu, clock) = (thread_time(), Instant::now());
            let picture = raster.render(&frame, pace.width);
            let (width, height, bgra) = (picture.width, picture.height, picture.bgra.to_vec());
            let cost = match (cpu, thread_time()) {
                (Some(before), Some(after)) => after.saturating_sub(before),
                _ => clock.elapsed(),
            };
            let finished = Finished {
                width,
                height,
                bgra,
                pace,
                cost,
            };
            (raster, finished)
        });
        self._drawing = Some(cx.spawn(async move |this, cx| {
            let (raster, finished) = drawing.await;
            let landed = this.update(cx, |stage, cx| {
                stage.raster = Some(raster);
                stage.pacer.record(finished.pace, finished.cost);
                stage.finished = Some(finished);
                cx.notify();
            });
            if landed.is_err() {
                // The stage went away while the picture was being drawn.
            }
        }));
    }

    /// Wakes the stage in `delay`, unless it has stopped being painted by then.
    fn wake_at(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self._wake = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let woken = this.update(cx, |stage, cx| {
                let shown = stage.painted.is_some_and(|at| at.elapsed() < HIDDEN_AFTER);
                if shown {
                    cx.notify();
                }
            });
            if woken.is_err() {
                // The stage went away.
            }
        }));
    }

    /// The finished picture as the window's form of an image: the bytes are moved, not copied.
    fn take_finished(&mut self) -> Option<Arc<RenderImage>> {
        let done = self.finished.take()?;
        let image = image::RgbaImage::from_raw(done.width, done.height, done.bgra)?;
        Some(Arc::new(RenderImage::new(vec![image::Frame::new(image)])))
    }
}

/// The largest place of the canvas's proportions inside `bounds`, centred.
fn contained(bounds: Bounds<Pixels>, canvas: soyeht_avatar::Size) -> Bounds<Pixels> {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let fit = (w / canvas.width as f32).min(h / canvas.height as f32);
    let (shown_w, shown_h) = (canvas.width as f32 * fit, canvas.height as f32 * fit);
    Bounds::new(
        bounds.origin + point(px((w - shown_w) / 2.0), px((h - shown_h) / 2.0)),
        size(px(shown_w), px(shown_h)),
    )
}

impl Render for AvatarStage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(self.palette.face))
            .child(painted(cx.entity(), Self::paint).size_full())
    }
}

impl AvatarStage {
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.painted = Some(Instant::now());
        let shown = contained(bounds, self.avatar.package().canvas);
        if let Some(image) = self.take_finished() {
            swap_picture(window, &mut self.picture, image, |image| image);
        }
        if let Some(image) = &self.picture {
            paint_picture(window, shown, image);
        }
        self.keep_going(shown, window.scale_factor(), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    const SQUARE: soyeht_avatar::Size = soyeht_avatar::Size {
        width: 100,
        height: 100,
    };

    #[test]
    fn the_picture_is_centred_in_the_longer_side() {
        let shown = contained(bounds(10.0, 20.0, 400.0, 200.0), SQUARE);
        assert_eq!(shown, bounds(110.0, 20.0, 200.0, 200.0));
        let shown = contained(bounds(0.0, 0.0, 100.0, 300.0), SQUARE);
        assert_eq!(shown, bounds(0.0, 100.0, 100.0, 100.0));
    }

    #[test]
    fn a_resting_avatar_is_idle_and_silent() {
        let drive = Resting.drive(1.0 / 60.0);
        assert_eq!((drive.state, drive.audio), (State::Idle, None));
    }
}
