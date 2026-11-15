//! The two motion primitives the runtime is made of: a critically damped follower (eased in and
//! out, no overshoot) and an under-damped spring that overshoots a little and settles.
//!
//! Ported from the smooth-damp follower of `rig/runtime/mouth.js` and `static/sabio-acting.js`
//! (Apache-2.0, the same authors).

/// A value that chases a target with critically damped motion (the "SmoothDamp" polynomial
/// approximation of the exponential, so it never overshoots and stays cheap).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Follow {
    pub value: f64,
    pub vel: f64,
    pub target: f64,
    pub smooth: f64,
}

impl Follow {
    pub fn new(value: f64, smooth: f64) -> Self {
        Self {
            value,
            vel: 0.0,
            target: value,
            smooth,
        }
    }

    /// Advance towards the stored target with the stored smooth time.
    pub fn step(&mut self, dt: f64) -> f64 {
        self.step_toward(self.target, self.smooth, dt)
    }

    /// Advance towards `target` with smooth time `smooth` (seconds) and return the new value.
    pub fn step_toward(&mut self, target: f64, smooth: f64, dt: f64) -> f64 {
        let omega = 2.0 / smooth.max(1e-3);
        let x = omega * dt;
        let damp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
        let change = self.value - target;
        let temp = (self.vel + omega * change) * dt;
        self.vel = (self.vel - omega * temp) * damp;
        self.value = target + (change + temp) * damp;
        self.value
    }

    /// Slew limit: the value may not move more than `max_delta` from `before` in this step. The
    /// velocity is cut with it, so the follower does not wind up against the limit.
    pub fn limit_step(&mut self, before: f64, max_delta: f64, dt: f64) {
        if self.value - before > max_delta {
            self.value = before + max_delta;
            self.vel = self.vel.min(max_delta / dt);
        } else if before - self.value > max_delta {
            self.value = before - max_delta;
            self.vel = self.vel.max(-max_delta / dt);
        }
    }
}

/// An under-damped spring that impulses kick: a nod that overshoots a little and settles.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Kick {
    x: f64,
    v: f64,
    omega: f64,
    damping: f64,
}

impl Kick {
    pub fn new(freq: f64, damping: f64) -> Self {
        Self {
            x: 0.0,
            v: 0.0,
            omega: 2.0 * std::f64::consts::PI * freq,
            damping,
        }
    }

    pub fn kick(&mut self, velocity: f64) {
        self.v += velocity;
    }

    pub fn step(&mut self, dt: f64) -> f64 {
        let steps = (dt / 0.008).ceil().max(1.0);
        let h = dt / steps;
        for _ in 0..steps as u32 {
            self.v +=
                (-self.omega * self.omega * self.x - 2.0 * self.damping * self.omega * self.v) * h;
            self.x += self.v * h;
        }
        self.x
    }
}

/// Smoothstep: eased between 0 and 1.
pub(crate) fn ease(t: f64) -> f64 {
    let x = t.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
