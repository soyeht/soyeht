//! Which of the package's eye frames shows a look. The package carries the frames; which one a
//! look needs is a rule of the renderers, held here so every renderer draws the same eyes
//! (`eyeFrames` of `contracts/vectors/avatar-mouth.json`).

use crate::acting::{Eye, Gaze, Lid};

/// The id (one of `EYE_IDS`) of the frame to lay over the base picture, or `None` when the base
/// picture's own open eyes are the look. A closed lid is the blink and a half-closed one is
/// `half`, whatever the eyes look at; open eyes that look away from the centre are the frame of
/// that gaze; open eyes at the centre are `soft` while listening, and otherwise the base's own.
pub fn eye_frame(eye: Eye) -> Option<&'static str> {
    match eye.lid {
        Lid::Closed => Some("blink"),
        Lid::Half => Some("half"),
        Lid::Open => match eye.gaze {
            Gaze::Left => Some("left"),
            Gaze::Right => Some("right"),
            Gaze::Up => Some("up"),
            Gaze::Down => Some("down"),
            Gaze::UpLeft => Some("upleft"),
            Gaze::Center => eye.soft.then_some("soft"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::EYE_IDS;

    #[test]
    fn every_frame_a_look_names_is_one_a_package_carries() {
        for gaze in [
            Gaze::Center,
            Gaze::Left,
            Gaze::Right,
            Gaze::Up,
            Gaze::Down,
            Gaze::UpLeft,
        ] {
            for lid in [Lid::Open, Lid::Half, Lid::Closed] {
                for soft in [false, true] {
                    let named = eye_frame(Eye { gaze, lid, soft });
                    assert!(named.is_none_or(|id| EYE_IDS.contains(&id)));
                }
            }
        }
    }
}
