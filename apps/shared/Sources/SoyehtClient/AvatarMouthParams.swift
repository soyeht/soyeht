import Foundation

/// The mouth as five continuous parameters, and the nine drawings as anchors in that space
/// (ADR 0166, section 6; the second implementation of `soyeht-avatar`'s `params`).
///
/// A mouth is never a picture. Phonemes are targets in this space; the parameters become morph
/// weights that blend the lip contours of the nine drawings, so exactly one mouth is ever
/// visible. Nothing here picks a drawing.
struct AvatarParams: Equatable, Sendable, Decodable {
  /// Opening (jaw height), each in 0...1.
  var o: Double
  /// Width: 0 pursed, 1 spread.
  var w: Double
  /// Rounding (lip protrusion).
  var r: Double
  /// Closure: lips pressed (M, B, P).
  var c: Double
  /// Lip-teeth contact (F, V).
  var f: Double

  /// The resting mouth: closed, a slight smile.
  static let rest = AvatarParams(o: 0, w: 0.5, r: 0, c: 0, f: 0)

  static let count = 5

  var array: [Double] { [o, w, r, c, f] }

  init(o: Double, w: Double, r: Double, c: Double, f: Double) {
    self.o = o
    self.w = w
    self.r = r
    self.c = c
    self.f = f
  }

  init(array a: [Double]) {
    self.init(o: a[0], w: a[1], r: a[2], c: a[3], f: a[4])
  }
}

func avatarClamp01(_ x: Double) -> Double { min(max(x, 0), 1) }

/// The nine drawings of a mouth. `rest` is the base picture's own mouth; the other eight are the
/// visemes. The order is the order of the weights and of the package's shapes.
enum AvatarShape: Int, CaseIterable, Sendable {
  case rest, closedMBP, a, e, i, o, u, fv, ldt

  /// The id the package and the frame spec use for this drawing.
  var id: String {
    switch self {
    case .rest: "rest"
    case .closedMBP: "closed_MBP"
    case .a: "A"
    case .e: "E"
    case .i: "I"
    case .o: "O"
    case .u: "U"
    case .fv: "FV"
    case .ldt: "LDT"
    }
  }

  /// Where this drawing sits in the parameter space.
  var anchor: AvatarParams {
    switch self {
    case .rest: AvatarParams(o: 0, w: 0.5, r: 0, c: 0, f: 0)
    case .closedMBP: AvatarParams(o: 0, w: 0.45, r: 0, c: 1, f: 0)
    case .a: AvatarParams(o: 1, w: 0.55, r: 0.1, c: 0, f: 0)
    case .e: AvatarParams(o: 0.4, w: 0.9, r: 0, c: 0, f: 0)
    case .i: AvatarParams(o: 0.15, w: 1, r: 0, c: 0, f: 0)
    case .o: AvatarParams(o: 0.65, w: 0.15, r: 0.8, c: 0, f: 0)
    case .u: AvatarParams(o: 0.2, w: 0, r: 1, c: 0, f: 0)
    case .fv: AvatarParams(o: 0.15, w: 0.7, r: 0, c: 0, f: 1)
    case .ldt: AvatarParams(o: 0.5, w: 0.65, r: 0, c: 0, f: 0)
    }
  }

  static let ids: [String] = allCases.map(\.id)
}

/// The morph weights of the eight visemes. `rest` takes what is left, so the nine always sum to 1
/// and are never negative.
struct AvatarMorphWeights: Equatable, Sendable {
  static let visemeCount = AvatarShape.allCases.count - 1

  /// `closed_MBP, A, E, I, O, U, FV, LDT`, in the order of `AvatarShape.allCases` after `rest`.
  var visemes: [Double]

  /// All nine weights in the order of `AvatarShape.allCases`, non-negative and summing to 1.
  var all: [Double] {
    var out = [Double](repeating: 0, count: AvatarShape.allCases.count)
    var sum = 0.0
    for (index, weight) in visemes.enumerated() {
      out[index + 1] = max(weight, 0)
      sum += out[index + 1]
    }
    if sum < 1 {
      out[0] = 1 - sum
    } else {
      out = out.map { $0 / sum }
    }
    return out
  }

  /// Parameters to morph weights: inverse-distance weights with a compact, continuous support.
  init(params q: AvatarParams) {
    let scale: [Double] = [1.3, 1.0, 1.0, 2.0, 2.2]
    let power = 2.5
    let keep = 5
    let qa = q.array
    let inverse: [Double] = AvatarShape.allCases.map { shape in
      let anchor = shape.anchor.array
      var d2 = 0.0
      for k in 0..<AvatarParams.count {
        let d = (qa[k] - anchor[k]) * scale[k]
        d2 += d * d
      }
      return 1.0 / pow(max(d2.squareRoot(), 1e-3), power)
    }
    let cut = inverse.sorted(by: >)[keep - 1]
    let raw = inverse.map { max($0 - cut, 0) }
    let total = raw.reduce(0, +)
    let sum = total == 0 ? 1 : total
    visemes = raw.dropFirst().map { $0 / sum }
  }

  init(visemes: [Double]) { self.visemes = visemes }
}
