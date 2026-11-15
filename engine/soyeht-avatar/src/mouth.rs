//! The mouth dynamics: five parameters that follow their targets with critically damped motion
//! (fast attack, slower release) and a slew limit, and the morph weights that follow the
//! parameters' weights through one more critically damped filter.
//!
//! The filter is linear, so the weights keep summing to 1: the mouth passes between shapes by
//! mixing their geometry and never through a third shape, and it never steps. There is no code
//! path in this crate that picks a drawing; the only output is parameters and weights.
//! Ported from `createMouth` of `rig/runtime/mouth.js` (Apache-2.0, the same authors).

use crate::params::{
    DIMS, MorphWeights, Params, SHAPES, Shape, VISEMES, clamp01, weights_from_params,
};
use crate::smooth::Follow;
use crate::tune::Tune;

/// The state of one mouth.
#[derive(Debug, Clone)]
pub struct Mouth {
    tune: Tune,
    dims: [Follow; DIMS],
    weights: [Follow; SHAPES],
    /// Seconds since something was last said. Infinite at the start: the lips stay at rest and do
    /// not close before the first word.
    silent: f64,
    q: Params,
}

impl Mouth {
    pub fn new(tune: Tune) -> Self {
        let rest = Params::REST.to_array();
        let dims = std::array::from_fn(|k| Follow::new(rest[k], 0.1));
        let weights = std::array::from_fn(|k| Follow::new(if k == 0 { 1.0 } else { 0.0 }, 0.1));
        Self {
            tune,
            dims,
            weights,
            silent: f64::INFINITY,
            q: Params::REST,
        }
    }

    /// The parameters after the last step.
    pub fn params(&self) -> Params {
        self.q
    }

    /// The morph weights after the last step.
    pub fn weights(&self) -> MorphWeights {
        let mut visemes = [0.0; VISEMES];
        for (slot, follow) in visemes.iter_mut().zip(&self.weights[1..]) {
            *slot = follow.value.max(0.0);
        }
        MorphWeights { visemes }
    }

    /// Advance by `dt` seconds towards `target` (`None` is silence) and return the new parameters.
    pub fn step(&mut self, dt: f64, target: Option<Params>) -> Params {
        let dt = dt.clamp(1e-4, 0.1);
        let tune = &self.tune;
        let goal = match target {
            Some(goal) => {
                self.silent = 0.0;
                goal
            }
            None => {
                self.silent += dt;
                if self.silent < tune.silence_close {
                    Params {
                        o: self.q.o * 0.4,
                        ..self.q
                    }
                } else if self.silent < tune.silence_rest {
                    Params::new(0.0, 0.5, 0.0, tune.pause_closure, 0.0)
                } else {
                    Params::REST
                }
            }
        };
        let relaxing = target.is_none() && self.silent >= tune.silence_rest;
        let rest = Params::REST.to_array();
        let goal = goal.to_array();
        let mut out = [0.0; DIMS];
        for k in 0..DIMS {
            let state = &mut self.dims[k];
            let opening = (goal[k] - rest[k]).abs() > (state.value - rest[k]).abs();
            let smooth = if relaxing {
                tune.relax
            } else if opening {
                tune.attack[k]
            } else {
                tune.release[k]
            };
            let before = state.value;
            state.step_toward(goal[k], smooth, dt);
            // Slew limit: no jump in one frame.
            let limit = tune.max_rate[k] * dt * if relaxing { 0.5 } else { 1.0 };
            state.limit_step(before, limit, dt);
            out[k] = clamp01(state.value);
        }
        self.q = Params::from_array(out);
        self.step_weights(dt);
        self.q
    }

    fn step_weights(&mut self, dt: f64) {
        let goal = weights_from_params(self.q);
        let smooth = if self.silent >= self.tune.silence_rest {
            self.tune.relax * 0.6
        } else {
            self.tune.weight_smooth
        };
        let rest_goal = 1.0 - goal.visemes.iter().sum::<f64>();
        self.weights[Shape::Rest.index()].step_toward(rest_goal, smooth, dt);
        for (follow, target) in self.weights[1..].iter_mut().zip(goal.visemes) {
            follow.step_toward(target, smooth, dt);
        }
    }
}
