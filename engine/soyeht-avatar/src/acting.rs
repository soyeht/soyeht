//! The acting layer: the four states of the avatar (idle, listening, thinking, speaking) and the
//! small, eased motions that make a still picture feel alive: blinks with in-betweens, saccades,
//! brows, nods, tilts and breathing. It returns the pose of the head and the choice of eye
//! drawing for the frame; the mouth is `mouth::Mouth`.
//!
//! Ported from `static/sabio-acting.js` (Apache-2.0, the same authors), in seconds, with a seeded
//! generator in place of `Math.random` so the same seed gives the same life in every
//! implementation. Every number worth tuning is in `ActingTune`.

use crate::acting_tune::ActingTune;
use crate::rng::Rng;
use crate::smooth::{Follow, Kick, ease};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

/// Where the eyes look: the choice of the eye drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gaze {
    Center,
    Left,
    Right,
    Up,
    Down,
    UpLeft,
}

impl Gaze {
    /// The unit direction the head follows a little.
    fn direction(self) -> (f64, f64) {
        match self {
            Gaze::Left => (-1.0, 0.0),
            Gaze::Right => (1.0, 0.0),
            Gaze::UpLeft => (-0.8, -1.0),
            Gaze::Up => (0.0, -1.0),
            Gaze::Down => (0.0, 1.0),
            Gaze::Center => (0.0, 0.0),
        }
    }
}

/// The lid drawing: the base picture's open eyes, the half-closed frame, or the blink frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lid {
    Open,
    Half,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Eye {
    pub gaze: Gaze,
    pub lid: Lid,
    /// The soft eyes of listening.
    pub soft: bool,
}

/// What the host tells the layer each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActingInput {
    /// The state the conversation is in (speaking follows the voice itself).
    pub state: State,
    /// Audio is playing now.
    pub voice: bool,
    /// The level of the audio now, in dB, when it plays.
    pub db: Option<f64>,
    /// The words are known (from the TTS timings), so `word_start` finds the accents and the
    /// level need not.
    pub words_known: bool,
    pub jaw: f64,
}

/// The pose the warp draws: pixels of the avatar's canvas and degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub rot: f64,
    pub head_x: f64,
    pub head_y: f64,
    pub face_x: f64,
    pub face_y: f64,
    pub brow_l: f64,
    pub brow_r: f64,
    pub brow_tilt: f64,
    pub jaw: f64,
    pub cheek: f64,
    pub breath: f64,
    pub zoom: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActingFrame {
    pub pose: Pose,
    pub eye: Eye,
    pub state: State,
}

/// A thing that happens a moment later.
#[derive(Debug, Clone, Copy)]
enum Action {
    /// The head follows the eyes, if they still look that way.
    FollowGaze(Gaze),
    /// Look back to the centre, if the eyes still look that way.
    CenterFrom(Gaze),
    BrowsBack,
    Blink,
    Nod(f64),
    HeadY(f64),
    /// The thinking look: away, brows up.
    Think,
}

struct Followers {
    rot: Follow,
    hx: Follow,
    hy: Follow,
    fx: Follow,
    fy: Follow,
    brow_l: Follow,
    brow_r: Follow,
    tilt: Follow,
    jaw: Follow,
    cheek: Follow,
    nx: Follow,
    ny: Follow,
}

pub struct Acting {
    tune: ActingTune,
    rng: Rng,
    f: Followers,
    nod: Kick,
    nod_rot: Kick,
    chest: Kick,
    clock: f64,
    state: State,
    timers: Vec<(f64, Action)>,
    eye: Eye,
    gaze_at: f64,
    blink: Option<(usize, f64)>,
    blink_at: f64,
    double_left: bool,
    away: Gaze,
    breath_phase: f64,
    voice_active: bool,
    voice_last: f64,
    voice_start: f64,
    silence_before: f64,
    db_ema: f64,
    stress_last: f64,
    brow_flick_at: f64,
    listen_nod_at: f64,
    sway_t: f64,
}

impl Acting {
    pub fn new(tune: ActingTune, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let gaze_at = rng.range_of(tune.gaze.idle_gap);
        let blink_at = rng.range_of(tune.blink.gap);
        let breath_phase = rng.unit();
        let brow_flick_at = rng.range(5.0, 9.0);
        let listen_nod_at = rng.range(2.5, 5.0);
        let sway_t = rng.unit() * 100.0;
        let f = Followers {
            rot: Follow::new(0.0, 0.35),
            hx: Follow::new(0.0, 0.4),
            hy: Follow::new(0.0, 0.4),
            fx: Follow::new(0.0, 0.55),
            fy: Follow::new(0.0, 0.55),
            brow_l: Follow::new(0.0, 0.14),
            brow_r: Follow::new(0.0, 0.14),
            tilt: Follow::new(0.0, 0.16),
            jaw: Follow::new(0.0, 0.045),
            cheek: Follow::new(0.0, 0.3),
            nx: Follow::new(0.0, 0.3),
            ny: Follow::new(0.0, 0.3),
        };
        Self {
            tune,
            rng,
            f,
            nod: Kick::new(3.4, 0.45),
            nod_rot: Kick::new(3.0, 0.5),
            chest: Kick::new(2.0, 0.6),
            clock: 0.0,
            state: State::Idle,
            timers: Vec::new(),
            eye: Eye {
                gaze: Gaze::Center,
                lid: Lid::Open,
                soft: false,
            },
            gaze_at,
            blink: None,
            blink_at,
            double_left: false,
            away: Gaze::UpLeft,
            breath_phase,
            voice_active: false,
            voice_last: -1e9,
            voice_start: -1e9,
            silence_before: 0.0,
            db_ema: -60.0,
            stress_last: -1e9,
            brow_flick_at,
            listen_nod_at,
            sway_t,
        }
    }

    fn after(&mut self, seconds: f64, action: Action) {
        self.timers.push((self.clock + seconds, action));
    }

    fn base_brow(&self) -> f64 {
        match self.state {
            State::Thinking => self.tune.brow.thinking,
            State::Listening => self.tune.brow.listening,
            _ => 0.0,
        }
    }

    fn do_blink(&mut self, double: bool) {
        if self.blink.is_none() {
            self.blink = Some((0, 0.0));
            self.double_left = double;
        }
    }

    fn look(&mut self, dir: Gaze, hold: f64) {
        if self.eye.gaze == dir {
            return;
        }
        self.eye.gaze = dir;
        // The eyes lead; the head follows a little later and comes back with the eyes.
        self.after(0.09, Action::FollowGaze(dir));
        if dir == Gaze::Center {
            self.f.nx.target = 0.0;
            self.f.ny.target = 0.0;
        }
        if dir != Gaze::Center && self.rng.unit() < self.tune.blink.on_gaze_shift {
            let delay = self.rng.range(0.02, 0.09);
            self.after(delay, Action::Blink);
        }
        if hold > 0.0 {
            self.after(hold, Action::CenterFrom(dir));
        }
    }

    fn raise_brows(&mut self, px: f64, hold: f64, asym: f64) {
        self.f.brow_l.target = px * (1.0 + asym);
        self.f.brow_r.target = px * (1.0 - asym);
        self.f.tilt.target = px * 0.35;
        self.after(hold, Action::BrowsBack);
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::FollowGaze(dir) => {
                if self.eye.gaze == dir {
                    let (dx, dy) = dir.direction();
                    self.f.nx.target = dx;
                    self.f.ny.target = dy;
                }
            }
            Action::CenterFrom(dir) => {
                if self.eye.gaze == dir {
                    self.look(Gaze::Center, 0.0);
                }
            }
            Action::BrowsBack => {
                let base = self.base_brow();
                self.f.brow_l.target = base;
                self.f.brow_r.target = base;
                self.f.tilt.target = if self.state == State::Thinking {
                    1.6
                } else {
                    0.0
                };
            }
            Action::Blink => self.do_blink(false),
            Action::Nod(v) => self.nod.kick(v),
            Action::HeadY(dy) => self.f.hy.target += dy,
            Action::Think => {
                if self.state == State::Thinking {
                    let away = self.away;
                    self.look(away, 0.0);
                    let px = self.tune.brow.thinking;
                    self.raise_brows(px, 0.7, 0.25);
                }
            }
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    fn set_state(&mut self, next: State) {
        if next == self.state {
            return;
        }
        let prev = self.state;
        self.state = next;
        let head = self.tune.head.clone();
        self.f.cheek.target = match next {
            State::Listening => 1.3,
            State::Idle => 0.5,
            _ => 0.0,
        };
        self.eye.soft = next == State::Listening;
        let pose = match next {
            State::Listening => Some((
                head.listen_tilt,
                head.listen_x,
                0.6,
                head.listen_x * 1.4,
                0.0,
            )),
            State::Thinking => Some((head.think_tilt, -1.0, head.think_y, -2.2, -1.8)),
            _ => None,
        };
        let (rot, hx, hy, fx, fy) = pose.unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0));
        self.f.rot.target = rot;
        self.f.hx.target = hx;
        self.f.hy.target = hy;
        self.f.fx.target = fx;
        self.f.fy.target = fy;
        match next {
            State::Listening => {
                let px = self.tune.brow.listening + 1.5;
                self.raise_brows(px, 0.5, 0.0);
                self.look(Gaze::Center, 0.0);
            }
            State::Thinking => {
                self.away = self.rng.pick(&[Gaze::UpLeft, Gaze::UpLeft, Gaze::Up]);
                self.after(0.12, Action::Think);
                self.after(0.06, Action::Blink);
                self.gaze_at = self.rng.range_of(self.tune.gaze.thinking_hold);
            }
            _ => {
                if self.eye.gaze != Gaze::Center {
                    self.look(Gaze::Center, 0.0);
                }
                if prev == State::Thinking {
                    self.after(0.04, Action::Blink);
                }
                if prev == State::Listening && next != State::Thinking {
                    self.nod.kick(80.0);
                }
            }
        }
    }

    fn sentence_start(&mut self) {
        // Anticipation (a small lift), then the nod, a quick inhale, brows up; sometimes a glance
        // away first.
        self.f.hy.target -= 1.2;
        self.after(0.095, Action::HeadY(1.2));
        self.nod.kick(-60.0);
        let nod = self.tune.head.nod_start * 11.0;
        self.after(0.11, Action::Nod(nod));
        self.chest.kick(9.0);
        let asym = self.rng.range(-0.25, 0.25);
        self.raise_brows(self.tune.brow.start, 0.42, asym);
        if self.silence_before > 0.9 && self.rng.unit() < self.tune.gaze.speaking_away_chance {
            let dir = self.rng.pick_weighted(&[
                (Gaze::Up, 3.0),
                (Gaze::Left, 2.0),
                (Gaze::Right, 2.0),
                (Gaze::Down, 1.0),
            ]);
            let hold = self.rng.range(0.35, 0.75);
            self.look(dir, hold);
        }
    }

    /// An accent of the given strength (1 is a plain one): a nod, a tilt, brows up.
    pub fn stress(&mut self, strength: f64) {
        self.nod.kick(self.tune.head.nod_speak * 9.0 * strength);
        let tilt = self.rng.range(-14.0, 14.0);
        self.nod_rot.kick(tilt);
        let asym = self.rng.range(-0.2, 0.2);
        self.raise_brows(self.tune.brow.stress * strength, 0.38, asym);
        if self.blink.is_none() {
            // Never blink in the middle of an accent.
            self.blink_at = self.blink_at.max(0.4);
        }
    }

    /// A word starts (from the TTS word timings): accent the content words, and lift the brows at
    /// the end of a question. `prev_ends_clause` says the word before ended a clause.
    pub fn word_start(
        &mut self,
        word: &str,
        index: usize,
        count: usize,
        prev_ends_clause: bool,
        text: &str,
    ) {
        let letters = word.chars().filter(|c| c.is_alphabetic()).count();
        let last = index + 1 == count;
        if last && text.trim_end().ends_with('?') {
            self.raise_brows(self.tune.brow.stress + 2.0, 0.9, 0.15);
            self.nod_rot.kick(-9.0);
            self.nod.kick(-8.0);
            return;
        }
        if word.ends_with('!') && letters >= 2 {
            self.stress(1.15);
            return;
        }
        let content = letters >= self.tune.stress.content_len || (prev_ends_clause && letters >= 4);
        if content
            && self.clock - self.stress_last > self.tune.stress.word_gap
            && self.rng.unit() < self.tune.stress.chance
        {
            self.stress_last = self.clock;
            let strength = 0.8 + self.rng.unit() * 0.5;
            self.stress(strength);
        }
    }

    /// Advance by `dt` seconds.
    pub fn step(&mut self, dt: f64, io: ActingInput) -> ActingFrame {
        let dt = dt.clamp(0.0, 0.1);
        self.clock += dt;
        self.sway_t += dt;
        if io.voice {
            if !self.voice_active {
                self.silence_before = self.clock - self.voice_last;
                self.voice_start = self.clock;
                if self.silence_before > 0.45 {
                    self.sentence_start();
                }
            }
            self.voice_last = self.clock;
        }
        self.voice_active = io.voice;
        let speaking =
            io.state == State::Speaking || io.voice || self.clock - self.voice_last < 0.5;
        self.set_state(if speaking { State::Speaking } else { io.state });

        // Stress is a voiced level clearly above the recent level.
        if let (true, Some(db)) = (io.voice, io.db) {
            let over = db - self.db_ema;
            if !io.words_known
                && over > self.tune.stress.db_over
                && self.clock - self.stress_last > self.tune.stress.min_gap
                && self.clock - self.voice_start > 0.12
            {
                self.stress_last = self.clock;
                self.stress((0.7 + (over - self.tune.stress.db_over) / 8.0).min(1.4));
            }
            self.db_ema += (db - self.db_ema) * (dt / 0.8).min(1.0);
        }

        for i in (0..self.timers.len()).rev() {
            if self.timers[i].0 <= self.clock {
                let (_, action) = self.timers.remove(i);
                self.run(action);
            }
        }
        self.saccade(dt, io.voice);
        self.blink_step(dt);
        self.small_life(dt);
        self.pose(dt, speaking, io.jaw)
    }

    /// Saccades: dart, hold, come back.
    fn saccade(&mut self, dt: f64, voiced: bool) {
        self.gaze_at -= dt;
        if self.gaze_at > 0.0 {
            return;
        }
        let hold_away = self.tune.gaze.hold_away;
        match self.state {
            State::Idle => {
                let dir = self.rng.pick_weighted(&[
                    (Gaze::Left, 3.0),
                    (Gaze::Right, 3.0),
                    (Gaze::Up, 2.0),
                    (Gaze::Down, 1.5),
                ]);
                let hold = self.rng.range_of(hold_away);
                self.look(dir, hold);
                self.gaze_at = self.rng.range_of(self.tune.gaze.idle_gap) + 0.7;
            }
            State::Listening => {
                if self.rng.unit() < 0.5 {
                    let dir = self.rng.pick_weighted(&[
                        (Gaze::Down, 3.0),
                        (Gaze::Left, 1.0),
                        (Gaze::Right, 1.0),
                    ]);
                    let hold = self.rng.range(0.35, 0.8);
                    self.look(dir, hold);
                }
                self.gaze_at = self.rng.range(2.5, 5.5);
            }
            State::Thinking => {
                let dir = self.rng.pick_weighted(&[
                    (Gaze::UpLeft, 3.0),
                    (Gaze::Up, 2.0),
                    (Gaze::Left, 1.5),
                ]);
                self.look(dir, 0.0);
                self.gaze_at = self.rng.range_of(self.tune.gaze.thinking_hold);
            }
            State::Speaking => {
                if !voiced && self.rng.unit() < 0.3 {
                    let dir = self.rng.pick(&[
                        Gaze::Up,
                        Gaze::Up,
                        Gaze::Left,
                        Gaze::Left,
                        Gaze::Right,
                        Gaze::Right,
                    ]);
                    let hold = self.rng.range(0.3, 0.6);
                    self.look(dir, hold);
                }
                self.gaze_at = self.rng.range(1.4, 3.5);
            }
        }
    }

    /// Blinks: three drawings (half, closed, half), never during an accent.
    fn blink_step(&mut self, dt: f64) {
        self.blink_at -= dt;
        if self.blink_at <= 0.0 && self.blink.is_none() {
            let double = self.rng.unit() < self.tune.blink.double_chance;
            self.do_blink(double);
            let gap = if self.state == State::Listening {
                self.tune.blink.gap_listening
            } else {
                self.tune.blink.gap
            };
            self.blink_at = self.rng.range_of(gap);
        }
        let Some((mut i, mut t)) = self.blink else {
            return;
        };
        let seq = self.tune.blink.seq;
        t += dt;
        if t >= seq[i] {
            t = 0.0;
            i += 1;
        }
        if i >= seq.len() {
            self.blink = None;
            self.eye.lid = Lid::Open;
            if self.double_left {
                self.double_left = false;
                self.after(0.12, Action::Blink);
            }
        } else {
            self.blink = Some((i, t));
            self.eye.lid = if i == 1 { Lid::Closed } else { Lid::Half };
        }
    }

    /// Small life: brow flicks when idle, nods while listening.
    fn small_life(&mut self, dt: f64) {
        self.brow_flick_at -= dt;
        if self.brow_flick_at <= 0.0 {
            if self.state == State::Idle {
                self.raise_brows(self.tune.brow.idle_flick, 0.26, 0.0);
            }
            self.brow_flick_at = self.rng.range(5.0, 10.0);
        }
        self.listen_nod_at -= dt;
        if self.listen_nod_at <= 0.0 {
            if self.state == State::Listening {
                self.nod.kick(self.tune.head.nod_listen * 9.0);
                if self.rng.unit() < 0.5 {
                    let v = self.tune.head.nod_listen * 6.0;
                    self.after(0.26, Action::Nod(v));
                }
            }
            self.listen_nod_at = self.rng.range(2.6, 5.5);
        }
    }

    /// Breathing (inhale faster than exhale) and the sway on periods that never line up.
    fn pose(&mut self, dt: f64, speaking: bool, jaw: f64) -> ActingFrame {
        let period = if speaking {
            self.tune.breath.period_speaking
        } else {
            self.tune.breath.period
        };
        self.breath_phase = (self.breath_phase + dt / period) % 1.0;
        let br = if self.breath_phase < 0.4 {
            ease(self.breath_phase / 0.4)
        } else {
            1.0 - ease((self.breath_phase - 0.4) / 0.6)
        };
        let k = if speaking {
            1.4
        } else if self.state == State::Listening {
            0.6
        } else {
            1.0
        };
        let tau = 2.0 * std::f64::consts::PI;
        let h = &self.tune.head;
        let t = self.sway_t;
        let sway_rot =
            k * h.sway_rot * ((t * tau / 7.3).sin() * 0.7 + (t * tau / 3.1 + 1.0).sin() * 0.3);
        let sway_x = k * h.sway_x * (t * tau / 8.9 + 2.0).sin();
        let sway_y = k * h.sway_y * (t * tau / 5.3).sin() - br * 0.9;
        self.f.jaw.target = jaw;
        let nodv = self.nod.step(dt);
        let nodr = self.nod_rot.step(dt);
        let ch = self.chest.step(dt);
        let f = &mut self.f;
        let (rot, hx, hy, fx, fy) = (
            f.rot.step(dt),
            f.hx.step(dt),
            f.hy.step(dt),
            f.fx.step(dt),
            f.fy.step(dt),
        );
        let (brow_l, brow_r, tilt, jaw, cheek) = (
            f.brow_l.step(dt),
            f.brow_r.step(dt),
            f.tilt.step(dt),
            f.jaw.step(dt),
            f.cheek.step(dt),
        );
        let (nx, ny) = (f.nx.step(dt), f.ny.step(dt));
        let pose = Pose {
            rot: rot + sway_rot + nodr * 0.12 + nodv * 0.05 + nx * 0.35,
            head_x: hx + sway_x + nx * 2.2,
            head_y: hy + sway_y + nodv * 0.9 + ny * 1.2,
            face_x: fx + sway_x * 0.8 + nx * 3.2,
            face_y: fy + nodv * 0.7 - br * 0.4 + ny * 1.8,
            brow_l,
            brow_r,
            brow_tilt: tilt,
            jaw,
            cheek,
            breath: (br * self.tune.breath.amount + ch * 0.05).clamp(0.0, 1.4),
            zoom: 1.0 + 0.003 * br + 0.0015 * (t * tau / 11.0).sin(),
        };
        ActingFrame {
            pose,
            eye: self.eye,
            state: self.state,
        }
    }
}
