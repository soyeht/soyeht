import Foundation
import SoyehtProtocol

@testable import SoyehtClient

/// Shared by the two vector files of the avatar (`avatar-mouth.json` and `avatar-package.json`),
/// which `soyeht-avatar` and the Swift runtime both pass.
enum AvatarVector {
  static let fps = 60.0

  /// The first place where two values differ: numbers within `tolerance`, everything else equal.
  static func difference(
    _ a: JsonValue, _ b: JsonValue, tolerance: Double, at path: String = ""
  ) -> String? {
    if let x = a.double, let y = b.double {
      return abs(x - y) <= tolerance ? nil : "\(path): \(x) against \(y)"
    }
    switch (a, b) {
    case (.array(let x), .array(let y)) where x.count == y.count:
      for (i, pair) in zip(x, y).enumerated() {
        if let why = difference(pair.0, pair.1, tolerance: tolerance, at: "\(path)/\(i)") {
          return why
        }
      }
      return nil
    case (.object(let x), .object(let y)) where x.count == y.count:
      for (key, value) in x.sorted(by: { $0.key < $1.key }) {
        guard let other = y[key] else { return "\(path)/\(key): missing" }
        if let why = difference(value, other, tolerance: tolerance, at: "\(path)/\(key)") {
          return why
        }
      }
      return nil
    default:
      return a == b ? nil : "\(path): \(a) against \(b)"
    }
  }

  static func numbers(_ xs: [Double]) -> JsonValue { .array(xs.map { .number($0) }) }

  static func phonemes(_ v: JsonValue) -> [AvatarPhoneme] {
    (v.array ?? []).map {
      AvatarPhoneme(
        p: $0["p"].string?.unicodeScalars.first ?? " ", t0: $0["t0"].double ?? 0,
        t1: $0["t1"].double ?? 0)
    }
  }

  static func phonemeValues(_ list: [AvatarPhoneme]) -> JsonValue {
    .array(
      list.map {
        .object(["p": .string(String($0.p)), "t0": .number($0.t0), "t1": .number($0.t1)])
      })
  }

  static func envelope(_ v: JsonValue) -> AvatarEnvelope? {
    guard let hop = v["hop"].double, let db = v["db"].array else { return nil }
    return AvatarEnvelope(db: db.compactMap(\.double), hop: hop)
  }

  /// `headRadii` of a case: the horizontal and vertical radius of the head, in pixels (the
  /// reference face when absent).
  static func radii(_ v: JsonValue) -> [Double] {
    let reference = AvatarActing.referenceHead
    let given = v["headRadii"].array ?? []
    return (0..<2).map {
      given.indices.contains($0) ? given[$0].double ?? reference[$0] : reference[$0]
    }
  }

  static func state(_ id: String?) -> AvatarState { id.flatMap(AvatarState.init) ?? .idle }

  static func seed(_ v: JsonValue) -> UInt64 {
    if case .integer(let n) = v["seed"] { return UInt64(max(n, 0)) }
    return 0
  }

  static func wordStarts(_ v: JsonValue) -> [AvatarWordStart] {
    (v.array ?? []).map {
      AvatarWordStart(
        t: $0["t"].double ?? 0, word: $0["word"].string ?? "",
        prevEndsClause: $0["prevEndsClause"].bool ?? false)
    }
  }
}
