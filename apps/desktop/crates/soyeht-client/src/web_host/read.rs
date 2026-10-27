//! `web.read`: reading a page the way a person does (ADR 0109, ADR 0110). The page is scrolled
//! with the engine's own wheel events, one screen at a time; at each screen the host reads the
//! visible text and links with a script that only reads, in the isolated world, and takes the
//! engine's own picture of the pane; the pictures are joined by the offsets the page reported.
//! The same walk makes the full-page screenshot, without the text.
//!
//! What the walk keeps to:
//! - the page is left where the person had it, on every way out (except when a person took the
//!   pane over, who is not scrolled again);
//! - a person's input is looked for before every scroll;
//! - when the document does not scroll but a large element inside it does, that one is scrolled;
//! - the walk ends if the pane is resized;
//! - a band pinned to an edge is cut from the screens that repeat it, so it leaves no seam;
//! - the reply stays inside the contract's bytes: it is cut, never refused.

use super::{
    HostError, Outcome, Page, attachment, clip,
    picture::{self, Picture},
    read_content::{Accumulator, Block},
    stitch::{self, Tile},
};
use ::image::{RgbImage, imageops};
use serde::Deserialize;
use serde_json::{Value, json};
use soyeht_protocol::{WebReadLink, limits};
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!("js/read.js");
/// The most pixels the joined picture holds.
const PIXEL_BUDGET: f64 = 4_000_000.0;
/// The share of a screen one scroll moves, so each screen overlaps the one before it.
const STEP: f64 = 0.85;
/// A first scroll is shorter, to learn how far the engine's wheel moves the page.
const FIRST_STEP: f64 = 0.5;
/// Lines of wheel before the first measure: one line is this many pixels in the engine.
const GUESS_LINE_PIXELS: f64 = 40.0;
/// What a measured line of the wheel may be, so one slow frame cannot throw the walk off.
const MIN_LINE_PIXELS: f64 = 5.0;
const MAX_LINE_PIXELS: f64 = 400.0;
/// A scroll that moves the page more than this share of a screen leaves a gap.
const OVERSHOOT: f64 = 0.98;
/// How long the walk waits for a scroll, and what it loads, to stop moving.
const SETTLE_POLL: Duration = Duration::from_millis(80);
const SETTLE_POLLS: u32 = 24;
/// How many polls in a row the page must be still to be taken as settled.
const STEADY_POLLS: u32 = 3;
/// How many times a picture is taken again when the page moved while it was taken.
const PICTURE_TRIES: u32 = 4;
/// How many scrolls it takes to go back to the top, or to the place the person left.
const RETURN_TRIES: u32 = 10;
/// A full-page screenshot gives up after this long.
const SCREENSHOT_BUDGET: Duration = Duration::from_secs(20);
/// The room the answer's other fields take of the byte budget of a read.
const ENVELOPE_BYTES: usize = 2048;

/// What the script says of the page on one screen.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Screen {
    inner: bool,
    scroll_top: f64,
    scroll_height: f64,
    view_width: f64,
    view_height: f64,
    top: f64,
    bottom: f64,
    left: f64,
    right: f64,
    fixed_top: f64,
    fixed_bottom: f64,
    #[serde(default)]
    blocks: Vec<Block>,
    #[serde(default)]
    links: Vec<WebReadLink>,
    /// The document held more text than the script reads.
    #[serde(default)]
    capped: bool,
}

impl Screen {
    /// The height of what scrolls, as the person sees it.
    fn area_height(&self) -> f64 {
        self.bottom - self.top
    }

    fn at_end(&self) -> bool {
        self.scroll_top + self.area_height() >= self.scroll_height - 1.0
    }

    fn same_geometry(&self, other: &Self) -> bool {
        (self.scroll_top - other.scroll_top).abs() < 0.5
            && (self.scroll_height - other.scroll_height).abs() < 0.5
            && (self.view_width - other.view_width).abs() < 0.5
            && (self.view_height - other.view_height).abs() < 0.5
    }

    /// Where a wheel over the scrolling part goes.
    fn center(&self) -> (f64, f64) {
        (
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        )
    }
}

/// What a walk of the page gave.
pub(super) struct Capture {
    png: Vec<u8>,
    text: String,
    links: Vec<WebReadLink>,
    screens: usize,
    end_of_page: bool,
    truncated: bool,
}

/// How far one line of the engine's wheel moves the page, learned on the first scroll.
struct Wheel {
    line_pixels: f64,
}

struct Walk<'a> {
    page: &'a dyn Page,
    wheel: Wheel,
}

impl Walk<'_> {
    fn screen(&self, collect: bool) -> Result<Screen, HostError> {
        let call = format!(
            "{}({collect},{})",
            SCRIPT.trim_end(),
            limits::WEB_READ_LINKS * 4
        );
        let answer = self.page.run_script(&call)?;
        serde_json::from_str(&answer)
            .map_err(|_| HostError::Internal("The page returned an invalid state."))
    }

    /// Waits for the scroll, and for what it loaded, to stop moving.
    fn settle(&self) -> Result<Screen, HostError> {
        let mut previous: Option<Screen> = None;
        let mut steady = 0;
        let mut latest = self.screen(false)?;
        for _ in 0..SETTLE_POLLS {
            std::thread::sleep(SETTLE_POLL);
            latest = self.screen(false)?;
            steady = match &previous {
                Some(before) if latest.same_geometry(before) => steady + 1,
                _ => 0,
            };
            if steady >= STEADY_POLLS {
                break;
            }
            previous = Some(latest.clone());
        }
        Ok(latest)
    }

    /// The picture of the pane once the page is where `screen` says it is and has painted what it
    /// shows: the page may still be moving when the script has answered, and the engine paints
    /// what a scroll revealed a little after, so a picture is taken until two in a row agree. A
    /// picture of another place, or of a half-painted one, would show as a seam. A page that never
    /// stops (a video, a carousel) is taken as it is after a few tries: a read is not refused for
    /// moving.
    fn still_picture(&self, screen: &Screen) -> Result<Picture, HostError> {
        let mut before = self.page.picture()?;
        for _ in 0..PICTURE_TRIES {
            std::thread::sleep(SETTLE_POLL);
            let after = self.page.picture()?;
            let steady = after.agrees_with(&before) && self.screen(false)?.same_geometry(screen);
            before = after;
            if steady {
                break;
            }
        }
        Ok(before)
    }

    /// One scroll of the wheel over the scrolling part, unless a person has the pane.
    fn scroll(&self, screen: &Screen, pixels: f64) -> Result<(), HostError> {
        if self.page.handoff().person_has_control() {
            return Err(super::Handoff::took_over());
        }
        let (x, y) = screen.center();
        self.page.wheel(x, y, pixels / self.wheel.line_pixels);
        Ok(())
    }

    /// Goes to `target` (a scroll offset of the same scrolling part), a scroll at a time.
    fn go_to(&mut self, target: f64) -> Result<(), HostError> {
        for _ in 0..RETURN_TRIES {
            let screen = self.settle()?;
            let distance = target - screen.scroll_top;
            if distance.abs() < 1.0 {
                return Ok(());
            }
            self.scroll(&screen, distance)?;
        }
        Ok(())
    }
}

/// Reads the page from the top, `limit` screens at most, until `deadline`.
pub(super) fn walk(
    page: &dyn Page,
    limit: usize,
    collect: bool,
    deadline: Instant,
) -> Result<Capture, HostError> {
    let mut walk = Walk {
        page,
        wheel: Wheel {
            line_pixels: GUESS_LINE_PIXELS,
        },
    };
    // The pane may still be taking its size (a card shown or hidden just before the command ran).
    let origin = walk.settle()?;
    let result = walk_screens(&mut walk, &origin, limit, collect, deadline);
    // The page goes back where the person had it, unless a person has taken the pane over.
    if !matches!(result, Err(HostError::HumanTookOver(_)))
        && let Ok(now) = walk.screen(false)
        && now.inner == origin.inner
        && walk.go_to(origin.scroll_top).is_err()
    {
        // A person took the pane while it went back, or the page stopped answering: it is left
        // where it is, and the read that was made stands.
    }
    result
}

fn walk_screens(
    walk: &mut Walk<'_>,
    origin: &Screen,
    limit: usize,
    collect: bool,
    deadline: Instant,
) -> Result<Capture, HostError> {
    let mut accumulator = Accumulator::default();
    let mut images: Vec<RgbImage> = Vec::new();
    let mut tiles: Vec<Tile> = Vec::new();
    let mut end_of_page = false;
    walk.go_to(0.0)?;
    let mut previous = -1.0;
    let mut calibrated = false;
    for index in 0..limit {
        let turn = Instant::now();
        if walk.page.handoff().person_has_control() {
            return Err(super::Handoff::took_over());
        }
        let screen = walk.settle()?;
        let screen = if collect { walk.screen(true)? } else { screen };
        if (screen.view_width - origin.view_width).abs() > 0.5
            || (screen.view_height - origin.view_height).abs() > 0.5
        {
            // The pane was resized: the screens no longer fit one another.
            return Err(HostError::Internal("The pane was resized during the read."));
        }
        if index > 0 && screen.scroll_top <= previous + 0.5 {
            break;
        }
        let at_end = screen.at_end();
        let picture = walk.still_picture(&screen)?;
        let (image, tile) = cut(&picture, &screen, index == 0, at_end)?;
        images.push(image);
        tiles.push(tile);
        accumulator.add(&screen.blocks, &screen.links);
        if screen.capped {
            accumulator.cut();
        }
        previous = screen.scroll_top;
        end_of_page = at_end;
        // Another screen is begun only when a screen as slow as this one, and the walk back, still
        // fit: the answer is due before the engine stops waiting for it.
        if at_end || index + 1 == limit || Instant::now() + turn.elapsed() * 2 >= deadline {
            break;
        }
        let room = screen.area_height() - screen.fixed_top - screen.fixed_bottom;
        let share = if calibrated { STEP } else { FIRST_STEP };
        let wanted = (room * share).max(24.0);
        walk.scroll(&screen, wanted)?;
        let moved = walk.settle()?.scroll_top - screen.scroll_top;
        if moved > 0.5 {
            // How far a line of the wheel moves the page, as measured, within reason.
            let measured = moved / (wanted / walk.wheel.line_pixels);
            walk.wheel.line_pixels = measured.clamp(MIN_LINE_PIXELS, MAX_LINE_PIXELS);
            calibrated = true;
        }
        if moved > room * OVERSHOOT {
            // The page went further than a screen: rows between the screens would be missing.
            // It goes back to where the step should have ended.
            walk.go_to(screen.scroll_top + room * STEP)?;
        }
    }
    finish(&images, &tiles, origin.view_width, accumulator, end_of_page)
}

/// The rows of `picture` that belong to this screen, and where they are on the page. Pinned bands
/// are kept on the first screen (the top one) and on the last (the bottom one) and cut from the
/// others, which would show them again.
fn cut(
    picture: &Picture,
    screen: &Screen,
    first: bool,
    last: bool,
) -> Result<(RgbImage, Tile), HostError> {
    let image = picture
        .to_rgb()
        .ok_or(HostError::Internal("The page snapshot could not be read."))?;
    let native = f64::from(image.height()) / screen.view_height;
    let top = if first {
        0.0
    } else if screen.inner {
        screen.top
    } else {
        screen.fixed_top
    };
    let bottom = if last {
        screen.bottom
    } else {
        screen.bottom - screen.fixed_bottom
    };
    let height = (bottom - top).max(1.0);
    // Rows of the picture, rounded and kept inside it.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (from, rows) = {
        let from = ((top * native).round().max(0.0) as u32).min(image.height().saturating_sub(1));
        let rows = ((height * native).round().max(1.0) as u32).min(image.height() - from);
        (from, rows)
    };
    let cropped = imageops::crop_imm(&image, 0, from, image.width(), rows).to_image();
    let area_top = if screen.inner { screen.top } else { 0.0 };
    Ok((
        cropped,
        Tile {
            offset: screen.scroll_top + (top - area_top),
            height,
        },
    ))
}

fn finish(
    images: &[RgbImage],
    tiles: &[Tile],
    view_width: f64,
    mut accumulator: Accumulator,
    end_of_page: bool,
) -> Result<Capture, HostError> {
    let joined = stitch::join(images, tiles, view_width, PIXEL_BUDGET).ok_or(
        HostError::Internal("The page snapshot could not be joined."),
    )?;
    let (png, cut) = picture::png_within_budget(joined)?;
    if cut {
        accumulator.cut();
    }
    let screens = images.len();
    let (text, links, truncated) = accumulator.into_parts();
    Ok(Capture {
        png,
        text,
        links,
        screens,
        end_of_page,
        truncated,
    })
}

/// A `web.read`.
pub(super) fn read(
    page: &dyn Page,
    pane_id: &str,
    args: &Value,
    timeout_ms: u32,
) -> Result<Outcome, HostError> {
    let screens = args
        .get("screens")
        .and_then(Value::as_u64)
        .map_or(limits::WEB_READ_SCREENS_DEFAULT, |screens| {
            usize::try_from(screens).unwrap_or(limits::WEB_READ_SCREENS)
        })
        .clamp(1, limits::WEB_READ_SCREENS);
    let budget = Duration::from_millis(u64::from(timeout_ms) * 4 / 5);
    let capture = walk(page, screens, true, Instant::now() + budget)?;
    let info = page.info();
    let reference = attachment(&capture.png);
    let mut value = json!({
        "navigationGeneration": page.generation().to_string(),
        "title": clip(&info.title, limits::WEB_TITLE_BYTES),
        "url": clip(&info.url, limits::WEB_FIELD_BYTES),
        "screens": capture.screens,
        "text": capture.text,
        "links": capture.links,
        "endOfPage": capture.end_of_page,
        "truncated": capture.truncated,
        "image": reference,
    });
    fit(&mut value, capture.png.len());
    let mut outcome = Outcome::ok(pane_id, value);
    outcome.attachment = Some((reference, capture.png));
    Ok(outcome)
}

/// A full-page screenshot: the same walk, the picture alone.
pub(super) fn full_page(page: &dyn Page, pane_id: &str) -> Result<Outcome, HostError> {
    let capture = walk(
        page,
        limits::WEB_READ_SCREENS,
        false,
        Instant::now() + SCREENSHOT_BUDGET,
    )?;
    let reference = attachment(&capture.png);
    let mut outcome = Outcome::ok(
        pane_id,
        json!({"image": reference, "endOfPage": capture.end_of_page}),
    );
    outcome.attachment = Some((reference, capture.png));
    Ok(outcome)
}

/// Cuts the links and then the text until the answer fits the bytes the contract allows it, and
/// the message of the harness that gets it with its image (`image` bytes of PNG, sent as base64),
/// and says it did. A read is never refused for being long.
fn fit(value: &mut Value, image: usize) {
    let in_message =
        limits::ACP_MESSAGE_BYTES.saturating_sub(image.div_ceil(3) * 4 + ENVELOPE_BYTES);
    let budget = (limits::WEB_READ_BYTES - ENVELOPE_BYTES).min(in_message);
    let size = |value: &Value| value.to_string().len();
    let mut cut = false;
    while size(value) > budget {
        cut = true;
        let removed = value["links"].as_array_mut().and_then(Vec::pop).is_some();
        if removed {
            continue;
        }
        let text = value["text"].as_str().unwrap_or_default();
        if text.is_empty() {
            break;
        }
        let keep = clip(text, text.len() / 2).to_owned();
        value["text"] = Value::String(keep);
    }
    if cut {
        value["truncated"] = Value::Bool(true);
    }
}

#[cfg(test)]
mod tests;
