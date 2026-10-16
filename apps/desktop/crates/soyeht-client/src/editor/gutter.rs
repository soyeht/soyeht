//! How wide the line-number gutter is (`EditorGutter` on the Mac; the cases are
//! `contracts/vectors/editor.json`, which both clients pass).
//!
//! The width follows the digits of the file's last line number, so that no number is clipped: a
//! line number is as wide as `digits * digit_width`, with the text inset on both sides, and the
//! gutter never gets narrower than its minimum (the `gutterWidth` token).

/// The decimal digits of the last line number of a file of `lines` lines; an empty file has one.
fn digits(lines: u32) -> u32 {
    lines.max(1).ilog10() + 1
}

/// The width of the gutter of a file of `lines` lines, in points, where a digit is `digit_width`
/// wide as the font was measured, `minimum` is the least the gutter is and `inset` the room kept
/// beside the numbers.
pub fn width(lines: u32, digit_width: f32, minimum: f32, inset: f32) -> f32 {
    ((digits(lines) as f32 * digit_width).ceil() + 2.0 * inset).max(minimum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::vector;

    #[test]
    fn the_gutter_is_as_wide_as_the_vector_says() {
        let gutter = &vector("editor")["gutterWidth"];
        let float = |value: &serde_json::Value| value.as_f64().unwrap() as f32;
        let (minimum, inset) = (float(&gutter["minimum"]), float(&gutter["inset"]));
        for case in gutter["cases"].as_array().unwrap() {
            let lines = u32::try_from(case["lineCount"].as_u64().unwrap()).unwrap();
            let wide = width(lines, float(&case["digitWidth"]), minimum, inset);
            assert_eq!(wide, float(&case["width"]), "{}", case["name"]);
        }
    }
}
