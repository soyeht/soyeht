use super::*;
use crate::web_host::{Handoff, PageInfo, Target};
use std::sync::Mutex;

/// The width of the pane in logical pixels, and how many device pixels one of them is.
const WIDTH: f64 = 100.0;
const NATIVE: u32 = 2;
/// How far one line of the model's wheel moves the page.
const LINE: f64 = 53.0;

/// A page of `height` pixels that the model scrolls as the engine would, and shows as a picture
/// whose every row says which row of the page it is.
struct Model {
    height: f64,
    view: Mutex<(f64, f64)>,
    scroll: Mutex<f64>,
    /// A band pinned to the top, painted on every screen.
    fixed_top: f64,
    /// The page scrolls inside an element that starts this far down the pane.
    inner_top: Option<f64>,
    handoff: Handoff,
    /// A person takes the pane at this scroll count.
    take_over_at: Option<usize>,
    /// The pane is resized at this scroll count.
    resize_at: Option<usize>,
    scrolls: Mutex<usize>,
    traveled: Mutex<Vec<f64>>,
    /// How far one line of the wheel moves the page.
    line: f64,
}

impl Model {
    fn new(height: f64) -> Self {
        Self {
            height,
            view: Mutex::new((WIDTH, 600.0)),
            scroll: Mutex::new(0.0),
            fixed_top: 0.0,
            inner_top: None,
            handoff: Handoff::default(),
            take_over_at: None,
            resize_at: None,
            scrolls: Mutex::new(0),
            traveled: Mutex::new(Vec::new()),
            line: LINE,
        }
    }

    fn at(&self) -> f64 {
        *self.scroll.lock().unwrap()
    }

    /// What the page row `row` looks like: a tone that tells it from the rows around it.
    fn tone(row: f64) -> u8 {
        (row / 3.0).floor() as u32 as u8 % 200 + 20
    }

    fn area(&self) -> (f64, f64) {
        let view = self.view.lock().unwrap().1;
        (self.inner_top.unwrap_or(0.0), view)
    }
}

impl Page for Model {
    fn incognito(&self) -> bool {
        false
    }
    fn navigate(&self, _: &Target) -> Result<(), HostError> {
        Ok(())
    }
    fn info(&self) -> PageInfo {
        PageInfo::default()
    }
    fn generation(&self) -> u64 {
        1
    }
    fn run_script(&self, source: &str) -> Result<String, HostError> {
        assert!(source.contains("fixedTop"), "{source}");
        let (width, view) = *self.view.lock().unwrap();
        let (top, bottom) = self.area();
        let scroll = self.at();
        let rows = (self.height / 200.0).ceil() as u32;
        let blocks: Vec<Value> = (0..rows)
            .filter(|row| {
                let y = f64::from(*row) * 200.0 - scroll;
                y + 200.0 > top && y < bottom
            })
            .map(|row| json!({"y": row * 200, "text": format!("row {row}"), "pinned": false}))
            .collect();
        Ok(json!({
            "inner": self.inner_top.is_some(), "scrollTop": scroll, "scrollHeight": self.height,
            "viewWidth": width, "viewHeight": view, "top": top, "bottom": view,
            "left": 0, "right": width, "fixedTop": self.fixed_top, "fixedBottom": 0,
            "blocks": blocks, "links": [{"text": "Home", "href": "https://example.test/"}]
        })
        .to_string())
    }
    fn picture(&self) -> Result<Picture, HostError> {
        let (_, view) = *self.view.lock().unwrap();
        let scroll = self.at();
        let (top, _) = self.area();
        let rows = view as u32 * NATIVE;
        let columns = WIDTH as u32 * NATIVE;
        let mut bytes = Vec::new();
        for row in 0..rows {
            let logical = f64::from(row) / f64::from(NATIVE);
            let tone = if logical < self.fixed_top {
                250
            } else {
                Self::tone(scroll + logical - top.min(logical))
            };
            for _ in 0..columns {
                bytes.extend_from_slice(&[tone, tone, tone, 255]);
            }
        }
        Ok(Picture {
            width: columns,
            height: rows,
            stride: columns as usize * 4,
            bytes,
        })
    }
    fn wheel(&self, _: f64, _: f64, lines: f64) {
        let count = {
            let mut scrolls = self.scrolls.lock().unwrap();
            *scrolls += 1;
            *scrolls
        };
        if self.take_over_at == Some(count) {
            self.handoff.note_person();
        }
        if self.resize_at == Some(count) {
            self.view.lock().unwrap().1 = 480.0;
        }
        let (top, bottom) = self.area();
        let most = (self.height - (bottom - top)).max(0.0);
        let mut scroll = self.scroll.lock().unwrap();
        *scroll = (*scroll + lines * self.line).clamp(0.0, most);
        self.traveled.lock().unwrap().push(*scroll);
    }
    fn handoff(&self) -> &Handoff {
        &self.handoff
    }
    fn has_cookies(&self, _: &str) -> Result<bool, HostError> {
        Ok(false)
    }
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(120)
}

/// Each of the page rows is where the joined picture shows it.
fn rows_are_in_place(image: &RgbImage, rows: &[f64], label: &str) {
    for row in rows {
        let y = (row * f64::from(NATIVE)) as u32;
        assert_eq!(
            image.get_pixel(5, y)[0],
            Model::tone(*row),
            "{label} row {row}"
        );
    }
}

fn joined(capture: &Capture) -> RgbImage {
    ::image::load_from_memory(&capture.png).unwrap().to_rgb8()
}

#[test]
fn a_long_page_is_one_picture_with_every_row_at_its_own_place() {
    let page = Model::new(2_000.0);
    let capture = walk(&page, 12, true, soon()).unwrap();
    assert!(capture.end_of_page && !capture.truncated);
    assert!(capture.screens >= 3, "{}", capture.screens);
    let image = joined(&capture);
    assert_eq!(image.width(), WIDTH as u32 * NATIVE);
    assert_eq!(image.height(), 2_000 * NATIVE);
    rows_are_in_place(
        &image,
        &[0.0, 300.0, 599.0, 600.0, 900.0, 1_400.0, 1_999.0],
        "",
    );
    assert!(capture.text.starts_with("row 0\nrow 1\nrow 2"));
    assert!(capture.text.ends_with("row 9"));
    assert_eq!(capture.links.len(), 1);
}

#[test]
fn the_page_is_left_where_the_person_had_it() {
    let page = Model::new(2_000.0);
    *page.scroll.lock().unwrap() = 700.0;
    walk(&page, 12, true, soon()).unwrap();
    assert!((page.at() - 700.0).abs() < 1.0, "{}", page.at());
}

#[test]
fn a_read_that_stops_at_its_screen_limit_says_the_page_did_not_end() {
    let page = Model::new(6_000.0);
    let capture = walk(&page, 3, true, soon()).unwrap();
    assert_eq!(capture.screens, 3);
    assert!(!capture.end_of_page);
    assert!(
        page.at() < 1.0,
        "back at the top the person left it: {}",
        page.at()
    );
}

#[test]
fn a_person_who_takes_the_pane_stops_the_read_and_is_not_scrolled_again() {
    let mut page = Model::new(6_000.0);
    page.take_over_at = Some(3);
    let result = walk(&page, 12, true, soon());
    assert!(matches!(result, Err(HostError::HumanTookOver(_))));
    let scrolls = *page.scrolls.lock().unwrap();
    assert_eq!(scrolls, 3, "no scroll after the person took over");
}

#[test]
fn the_person_is_looked_for_before_the_first_scroll_of_the_return_too() {
    let page = Model::new(2_000.0);
    page.handoff.note_person();
    assert!(matches!(
        walk(&page, 12, true, soon()),
        Err(HostError::HumanTookOver(_))
    ));
    assert_eq!(*page.scrolls.lock().unwrap(), 0);
}

#[test]
fn a_pane_resized_during_the_read_fails_it_and_the_page_is_left_where_it_was() {
    let mut page = Model::new(6_000.0);
    page.resize_at = Some(2);
    *page.scroll.lock().unwrap() = 300.0;
    let result = walk(&page, 12, true, soon());
    assert!(matches!(result, Err(HostError::Internal(message)) if message.contains("resized")));
    assert!((page.at() - 300.0).abs() < 1.0, "{}", page.at());
}

#[test]
fn a_pinned_band_is_on_the_picture_once_at_the_top() {
    let mut page = Model::new(2_000.0);
    page.fixed_top = 60.0;
    let capture = walk(&page, 12, true, soon()).unwrap();
    let image = joined(&capture);
    assert_eq!(image.get_pixel(5, 10)[0], 250, "the band at the top");
    for row in [100.0, 700.0, 1_300.0, 1_900.0] {
        let y = (row * f64::from(NATIVE)) as u32;
        assert_eq!(image.get_pixel(5, y)[0], Model::tone(row), "page row {row}");
    }
    // Every row under the first band is a row of the page: none of the band's tone comes again.
    let again = (120..image.height())
        .filter(|row| image.get_pixel(5, *row)[0] == 250)
        .count();
    assert_eq!(again, 0);
}

#[test]
fn an_element_that_scrolls_inside_the_page_is_the_one_scrolled() {
    let mut page = Model::new(1_800.0);
    page.inner_top = Some(50.0);
    let capture = walk(&page, 12, true, soon()).unwrap();
    assert!(capture.end_of_page);
    assert!(capture.screens >= 3);
    let image = joined(&capture);
    // The part of the pane above the scrolling element is the first picture's, and what the
    // element holds follows it row after row.
    assert!(image.height() > 1_700 * NATIVE);
}

#[test]
fn the_answer_is_cut_to_the_bytes_of_the_contract_and_says_so() {
    let mut value = json!({
        "text": "\"".repeat(limits::WEB_READ_TEXT_BYTES),
        "links": (0..100).map(|index| json!({"text": "t".repeat(256), "href": format!("https://e.test/{}{}", index, "\"".repeat(4_000))})).collect::<Vec<_>>(),
        "truncated": false,
    });
    assert!(value.to_string().len() > limits::WEB_READ_BYTES);
    fit(&mut value, 0);
    assert!(value.to_string().len() <= limits::WEB_READ_BYTES);
    assert_eq!(value["truncated"], true);
    let mut small = json!({"text": "hi", "links": [], "truncated": false});
    fit(&mut small, 0);
    assert_eq!(small["truncated"], false);
    // With a large picture beside it, what is left of the harness's message is what it may take.
    let mut beside =
        json!({"text": "x".repeat(limits::WEB_READ_TEXT_BYTES), "links": [], "truncated": false});
    fit(&mut beside, limits::WEB_AGENT_IMAGE_BYTES);
    assert!(
        (beside.to_string().len() + limits::WEB_AGENT_IMAGE_BYTES.div_ceil(3) * 4) * 2
            <= limits::ACP_MESSAGE_BYTES
    );
}

#[test]
fn a_wheel_that_moves_the_page_further_than_asked_leaves_no_gap() {
    for line in [9.0, 300.0] {
        let mut page = Model::new(2_000.0);
        page.line = line;
        let capture = walk(&page, 12, true, soon()).unwrap();
        assert!(capture.end_of_page, "line {line}");
        let image = joined(&capture);
        assert_eq!(image.height(), 2_000 * NATIVE, "line {line}");
        let rows = [250.0, 590.0, 610.0, 1_000.0, 1_333.0, 1_750.0];
        rows_are_in_place(&image, &rows, &format!("line {line}"));
    }
}
