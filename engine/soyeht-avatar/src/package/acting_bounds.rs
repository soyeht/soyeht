//! The range of every constant of the acting layer a package can set, each with its reason. The
//! acting layer runs on whatever a package says, and a stranger's package must not be able to
//! make the head leave the picture or a number leave the real line: a value of `1e308` makes the
//! pose not a number, and `5000` moves the head 7000 pixels. Distances are in reference pixels (a
//! 1024 pixel picture, see `REFERENCE_SIZE`), times in seconds, angles in degrees.

use serde_json::Value;

use crate::acting_tune::ActingTune;

/// `(pointer, least, most, reason)` for every number of an `ActingTune`.
pub(crate) const BOUNDS: &[(&str, f64, f64, &str)] = &[
    // Times between and during movements: a tic is not a rhythm, a minute is a stare.
    (
        "/blink/gap/0",
        0.2,
        60.0,
        "a blink every 0.2 s is a tic, one a minute is a stare",
    ),
    ("/blink/gap/1", 0.2, 60.0, "as the shortest gap"),
    ("/blink/gapListening/0", 0.2, 60.0, "as the blink gap"),
    ("/blink/gapListening/1", 0.2, 60.0, "as the blink gap"),
    ("/blink/doubleChance", 0.0, 1.0, "a chance"),
    ("/blink/onGazeShift", 0.0, 1.0, "a chance"),
    (
        "/blink/seq/0",
        0.01,
        1.0,
        "a lid movement lasts 10 ms to 1 s",
    ),
    (
        "/blink/seq/1",
        0.01,
        1.0,
        "a lid movement lasts 10 ms to 1 s",
    ),
    (
        "/blink/seq/2",
        0.01,
        1.0,
        "a lid movement lasts 10 ms to 1 s",
    ),
    (
        "/gaze/idleGap/0",
        0.2,
        60.0,
        "a glance every 0.2 s is a tic, one a minute is a stare",
    ),
    ("/gaze/idleGap/1", 0.2, 60.0, "as the shortest gap"),
    (
        "/gaze/holdAway/0",
        0.05,
        10.0,
        "a glance away is held from a flick to ten seconds",
    ),
    ("/gaze/holdAway/1", 0.05, 10.0, "as the shortest hold"),
    ("/gaze/speakingAwayChance", 0.0, 1.0, "a chance"),
    (
        "/gaze/thinkingHold/0",
        0.2,
        60.0,
        "a thinking look is held a fifth of a second to a minute",
    ),
    ("/gaze/thinkingHold/1", 0.2, 60.0, "as the shortest hold"),
    // The head. Sways are slow and a few pixels; the lab's are 1.4 and 0.85 pixels and 0.45 degrees.
    (
        "/head/swayRot",
        0.0,
        5.0,
        "a sway over 5 degrees is a turn of the head",
    ),
    (
        "/head/swayX",
        0.0,
        50.0,
        "under 5 % of the reference picture",
    ),
    (
        "/head/swayY",
        0.0,
        50.0,
        "under 5 % of the reference picture",
    ),
    (
        "/head/listenTilt",
        -15.0,
        15.0,
        "a tilt of the head over 15 degrees is a different pose",
    ),
    (
        "/head/listenX",
        -50.0,
        50.0,
        "under 5 % of the reference picture",
    ),
    ("/head/thinkTilt", -15.0, 15.0, "as the listening tilt"),
    (
        "/head/thinkY",
        -50.0,
        50.0,
        "under 5 % of the reference picture",
    ),
    // A nod is a kick of a spring: the peak is about a third of the number in pixels (the lab's
    // are 9 to 14), and these keep it near 15 pixels at the most.
    (
        "/head/nodSpeak",
        0.0,
        40.0,
        "the peak of a nod is about a third of this, in pixels",
    ),
    ("/head/nodStart", 0.0, 40.0, "as nodSpeak"),
    ("/head/nodListen", 0.0, 40.0, "as nodSpeak"),
    (
        "/brow/stress",
        0.0,
        30.0,
        "a brow lifts a few pixels, never 3 % of the picture",
    ),
    ("/brow/start", 0.0, 30.0, "as the stress lift"),
    ("/brow/thinking", 0.0, 30.0, "as the stress lift"),
    ("/brow/listening", 0.0, 30.0, "as the stress lift"),
    ("/brow/idleFlick", 0.0, 30.0, "as the stress lift"),
    (
        "/breath/period",
        1.0,
        30.0,
        "a breath takes one to thirty seconds",
    ),
    ("/breath/periodSpeaking", 1.0, 30.0, "as the breath period"),
    (
        "/breath/amount",
        0.0,
        2.0,
        "the chest rises at most twice the lab's",
    ),
    (
        "/stress/dbOver",
        1.0,
        40.0,
        "an accent is a level over the recent one by 1 to 40 dB",
    ),
    (
        "/stress/minGap",
        0.2,
        30.0,
        "accents are spaced a fifth of a second to half a minute",
    ),
    (
        "/stress/contentLen",
        1.0,
        64.0,
        "a word has at least a letter and no more than a name",
    ),
    ("/stress/chance", 0.0, 1.0, "a chance"),
    (
        "/stress/wordGap",
        0.0,
        30.0,
        "accents on words are spaced up to half a minute",
    ),
    // The resting expressions are targets of the mouth: parameters in 0 to 1.
    ("/expression/listening/o", 0.0, 1.0, "a mouth parameter"),
    ("/expression/listening/w", 0.0, 1.0, "a mouth parameter"),
    ("/expression/listening/r", 0.0, 1.0, "a mouth parameter"),
    ("/expression/listening/c", 0.0, 1.0, "a mouth parameter"),
    ("/expression/listening/f", 0.0, 1.0, "a mouth parameter"),
    ("/expression/thinking/o", 0.0, 1.0, "a mouth parameter"),
    ("/expression/thinking/w", 0.0, 1.0, "a mouth parameter"),
    ("/expression/thinking/r", 0.0, 1.0, "a mouth parameter"),
    ("/expression/thinking/c", 0.0, 1.0, "a mouth parameter"),
    ("/expression/thinking/f", 0.0, 1.0, "a mouth parameter"),
];

/// The ranges whose first number may not pass the second.
const ORDERED: [&str; 5] = [
    "/blink/gap",
    "/blink/gapListening",
    "/gaze/idleGap",
    "/gaze/holdAway",
    "/gaze/thinkingHold",
];

/// The pointer of the first constant out of its range, or `None`.
pub(crate) fn first_out_of_range(acting: &ActingTune) -> Option<String> {
    let Ok(value) = serde_json::to_value(acting) else {
        return Some(String::new());
    };
    for (pointer, least, most, _) in BOUNDS {
        match value.pointer(pointer).and_then(Value::as_f64) {
            Some(v) if v.is_finite() && (*least..=*most).contains(&v) => {}
            _ => return Some((*pointer).to_string()),
        }
    }
    let number = |pointer: String| {
        value
            .pointer(&pointer)
            .and_then(Value::as_f64)
            .unwrap_or(f64::NAN)
    };
    ORDERED
        .iter()
        .find(|range| number(format!("{range}/0")) > number(format!("{range}/1")))
        .map(|range| (*range).to_string())
}

/// Every number of a tune, by pointer: the bounds table must name each, so a field added later
/// cannot escape it.
#[cfg(test)]
pub(crate) fn numbers_of(value: &Value, at: &str, out: &mut Vec<String>) {
    match value {
        Value::Number(_) => out.push(at.to_string()),
        Value::Array(a) => a
            .iter()
            .enumerate()
            .for_each(|(i, v)| numbers_of(v, &format!("{at}/{i}"), out)),
        Value::Object(o) => o
            .iter()
            .for_each(|(k, v)| numbers_of(v, &format!("{at}/{k}"), out)),
        _ => {}
    }
}
