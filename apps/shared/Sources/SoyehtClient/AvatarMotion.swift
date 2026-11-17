import Foundation

/// The motion primitives the avatar runtime is made of: a critically damped follower, an
/// under-damped spring, the eased step and the seeded generator (the Swift side of
/// `soyeht-avatar`'s `smooth` and `rng`).

/// A value that chases a target with critically damped motion (the "SmoothDamp" polynomial
/// approximation of the exponential, so it never overshoots and stays cheap).
struct AvatarFollow: Sendable {
  var value: Double
  var vel = 0.0
  var target: Double
  var smooth: Double

  init(value: Double, smooth: Double) {
    self.value = value
    self.target = value
    self.smooth = smooth
  }

  /// Advance towards the stored target with the stored smooth time.
  @discardableResult
  mutating func step(_ dt: Double) -> Double {
    stepToward(target, smooth: smooth, dt: dt)
  }

  /// Advance towards `target` with smooth time `smooth` (seconds) and return the new value.
  @discardableResult
  mutating func stepToward(_ target: Double, smooth: Double, dt: Double) -> Double {
    let omega = 2.0 / avatarMax(smooth, 1e-3)
    let x = omega * dt
    let damp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x)
    let change = value - target
    let temp = (vel + omega * change) * dt
    vel = (vel - omega * temp) * damp
    value = target + (change + temp) * damp
    return value
  }

  /// Slew limit: the value may not move more than `maxDelta` from `before` in this step. The
  /// velocity is cut with it, so the follower does not wind up against the limit.
  mutating func limitStep(before: Double, maxDelta: Double, dt: Double) {
    if value - before > maxDelta {
      value = before + maxDelta
      vel = avatarMin(vel, maxDelta / dt)
    } else if before - value > maxDelta {
      value = before - maxDelta
      vel = avatarMax(vel, -maxDelta / dt)
    }
  }
}

/// An under-damped spring that impulses kick: a nod that overshoots a little and settles.
struct AvatarKick: Sendable {
  private var x = 0.0
  private var v = 0.0
  private let omega: Double
  private let damping: Double

  init(frequency: Double, damping: Double) {
    omega = 2.0 * Double.pi * frequency
    self.damping = damping
  }

  mutating func kick(_ velocity: Double) { v += velocity }

  mutating func step(_ dt: Double) -> Double {
    let steps = avatarMax((dt / 0.008).rounded(.up), 1.0)
    let h = dt / steps
    for _ in 0..<Int(steps) {
      v += (-omega * omega * x - 2.0 * damping * omega * v) * h
      x += v * h
    }
    return x
  }
}

/// The larger of two numbers, and the other when one is not a number: Rust's `f64::max`. Swift's
/// `max` answers the first when the second is not a number, and a number that is not a number
/// goes on to the pose.
func avatarMax(_ a: Double, _ b: Double) -> Double { a.isNaN ? b : (b.isNaN ? a : max(a, b)) }

/// The smaller of two numbers, ignoring one that is not a number: Rust's `f64::min`.
func avatarMin(_ a: Double, _ b: Double) -> Double { a.isNaN ? b : (b.isNaN ? a : min(a, b)) }

/// Smoothstep: eased between 0 and 1.
func avatarEase(_ t: Double) -> Double {
  let x = min(max(t, 0), 1)
  return x * x * (3.0 - 2.0 * x)
}

/// A small deterministic generator (SplitMix64), so the same seed gives the same blinks and
/// glances in every implementation, which is what lets a vector file test the acting layer.
struct AvatarRng: Sendable {
  private var state: UInt64

  init(seed: UInt64) { state = seed }

  private mutating func nextU64() -> UInt64 {
    state = state &+ 0x9E37_79B9_7F4A_7C15
    var z = (state ^ (state >> 30)) &* 0xBF58_476D_1CE4_E5B9
    z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
    return z ^ (z >> 31)
  }

  /// A number in `[0, 1)` with 53 random bits.
  mutating func unit() -> Double {
    Double(nextU64() >> 11) / 9_007_199_254_740_992.0
  }

  mutating func range(_ lo: Double, _ hi: Double) -> Double { lo + unit() * (hi - lo) }

  mutating func range(of bounds: [Double]) -> Double { range(bounds[0], bounds[1]) }

  mutating func pick<T>(_ items: [T]) -> T {
    let i = Int(unit() * Double(items.count))
    return items[min(i, items.count - 1)]
  }

  /// One of `(value, weight)` with probability proportional to its weight.
  mutating func pickWeighted<T>(_ items: [(T, Double)]) -> T {
    var r = unit() * items.reduce(0) { $0 + $1.1 }
    for (value, weight) in items {
      r -= weight
      if r <= 0 { return value }
    }
    return items[0].0
  }
}

/// A double as an index: not a number and negative are 0, and the far end saturates, as Rust's
/// `as usize` does. A host may hand over anything, and a trap is not an answer.
func avatarIndex(_ x: Double) -> Int {
  if x.isNaN || x <= 0 { return 0 }
  if x >= 9.0e18 { return Int.max }
  return Int(x)
}
