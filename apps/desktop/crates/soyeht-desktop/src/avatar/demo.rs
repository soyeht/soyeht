//! A window with one avatar from a package folder, for looking at a package and at the frame
//! cost on a machine (`examples/avatar_stage.rs`). A script plays the conversation's four states
//! in turn, with the level of a voice while it speaks, so the energy-only mouth moves.

use super::view::{AvatarStage, Drive, Driver};
use crate::{gallery::open_centered, theme::ThemeName};
use gpui::{App, AppContext};
use gpui_platform::application;
use soyeht_avatar::{Audio, State};
use soyeht_avatar_raster::load_folder;
use soyeht_platform::log;
use std::{path::PathBuf, sync::Arc};

/// How long each state lasts, in the order the script plays them (seconds).
const SCRIPT: [(State, f64); 4] = [
    (State::Idle, 3.0),
    (State::Listening, 3.0),
    (State::Thinking, 3.0),
    (State::Speaking, 6.0),
];
/// The syllables a second of the made-up voice, its quietest and loudest level (dB).
const SYLLABLES: f64 = 3.5;
const QUIET_DB: f64 = -45.0;
const LOUD_DB: f64 = -18.0;
const WINDOW: (f32, f32) = (640.0, 640.0);

struct Script {
    clock: f64,
}

impl Driver for Script {
    fn drive(&mut self, step: f64) -> Drive {
        self.clock += step;
        let cycle: f64 = SCRIPT.iter().map(|(_, seconds)| seconds).sum();
        let mut at = self.clock % cycle;
        for (state, seconds) in SCRIPT {
            if at < seconds {
                let audio = (state == State::Speaking).then(|| {
                    let swell = 0.5 + 0.5 * (std::f64::consts::TAU * SYLLABLES * at).sin();
                    Audio {
                        position: at,
                        db: Some(QUIET_DB + (LOUD_DB - QUIET_DB) * swell),
                    }
                });
                return Drive { state, audio };
            }
            at -= seconds;
        }
        Drive {
            state: State::Idle,
            audio: None,
        }
    }
}

/// Opens one window with the avatar in `folder`, in `theme`, and runs until it closes.
pub fn run_stage(folder: PathBuf, theme: ThemeName) {
    let avatar = match load_folder(&folder) {
        Ok(avatar) => Arc::new(avatar),
        Err(error) => {
            log::failure("desktop.avatar_load_failed", &error);
            return;
        }
    };
    application().run(move |cx: &mut App| {
        open_centered(cx, WINDOW, move |_, cx| {
            cx.new(|_| AvatarStage::new(avatar, Script { clock: 0.0 }, 1, theme.palette()))
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_plays_each_state_in_turn_and_speaks_only_when_speaking() {
        let mut script = Script { clock: 0.0 };
        let mut seen = Vec::new();
        for _ in 0..(15 * 4) {
            let drive = script.drive(0.25);
            assert_eq!(drive.audio.is_some(), drive.state == State::Speaking);
            if seen.last() != Some(&drive.state) {
                seen.push(drive.state);
            }
        }
        assert_eq!(
            seen,
            [
                State::Idle,
                State::Listening,
                State::Thinking,
                State::Speaking,
                State::Idle
            ]
        );
    }
}
