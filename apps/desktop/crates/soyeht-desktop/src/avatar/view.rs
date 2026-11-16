//! The avatar's stage: a view that shows one avatar and keeps it alive.
//!
//! Each frame it asks its `Driver` what the avatar is doing (the conversation's state and the
//! audio that plays now), steps the runtime by the time that really passed, draws the picture with
//! the rasteriser at the size the view has on screen and paints it. It asks for the next frame
//! itself, so it animates for as long as it is shown.

use super::{art::Avatar, raster::Rasteriser};
use crate::{
    picture::{paint_picture, painted, swap_picture},
    theme::Palette,
};
use gpui::{
    Bounds, Context, IntoElement, ParentElement, Pixels, Render, RenderImage, Styled, Window, div,
    point, px, rgb, size,
};
use soyeht_avatar::{Audio, Runtime, State, Tune};
use std::{sync::Arc, time::Instant};

/// A frame is never longer than this, however long the window was away (seconds): the acting
/// layer is not told that a minute went by in one step.
const LONGEST_STEP: f64 = 0.1;
/// The first frame's step, before there is a frame before it (seconds).
const FIRST_STEP: f64 = 1.0 / 60.0;
/// The fewest and the most pixels across a picture is drawn: a view the size of a thumbnail is
/// not drawn smaller than this, and a large one is drawn by the video memory's own scaling.
const NARROWEST: u32 = 64;
const WIDEST: u32 = 1024;

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

pub struct AvatarStage {
    palette: &'static Palette,
    avatar: Arc<Avatar>,
    rasteriser: Rasteriser,
    runtime: Runtime,
    driver: Box<dyn Driver>,
    /// When the last frame was drawn.
    last: Option<Instant>,
    /// The picture on screen.
    picture: Option<Arc<RenderImage>>,
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
            rasteriser: Rasteriser::new(Arc::clone(&avatar)),
            avatar,
            driver: Box::new(driver),
            last: None,
            picture: None,
        }
    }

    /// Steps the avatar by the time since the last frame and draws its picture for a place of
    /// `shown` logical pixels, at `scale` device pixels for each.
    fn frame(&mut self, shown: Bounds<Pixels>, scale: f32) -> Option<Arc<RenderImage>> {
        let now = Instant::now();
        let step = self.last.replace(now).map_or(FIRST_STEP, |last| {
            now.duration_since(last)
                .as_secs_f64()
                .clamp(0.0, LONGEST_STEP)
        });
        let Drive { state, audio } = self.driver.drive(step);
        let frame = self.runtime.step(step, state, audio);
        let across = (f32::from(shown.size.width) * scale).ceil() as u32;
        let width = across
            .clamp(NARROWEST, WIDEST)
            .min(self.avatar.package().canvas.width);
        let picture = self.rasteriser.render(&frame, width);
        let image =
            image::RgbaImage::from_raw(picture.width, picture.height, picture.bgra.to_vec())?;
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
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, _: &mut Context<Self>) {
        let shown = contained(bounds, self.avatar.package().canvas);
        if let Some(image) = self.frame(shown, window.scale_factor()) {
            swap_picture(window, &mut self.picture, image, |image| image);
        }
        if let Some(image) = &self.picture {
            paint_picture(window, shown, image);
        }
        window.request_animation_frame();
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
