import Foundation
import SoyehtProtocol

@testable import SoyehtClient

/// Shared by the two vector files of the avatar (`avatar-mouth.json` and `avatar-package.json`),
/// which `soyeht-avatar` and the Swift runtime both pass.
enum AvatarVector {
  static let fps = 60.0

  /// The groups of `avatar-mouth.json` and of `avatar-package.json` the Swift tests run. A group
  /// the file gains that is in neither list fails `everyGroupOfTheVectorFilesIsRunHere`, so a
  /// rule another implementation pins is never left out here without being said.
  static let mouthGroups: Set = ["weights", "dynamics", "aligner", "acting", "runtime", "warp"]
  static let packageGroups: Set = ["cases", "mesh"]
  /// Lists of the files that are inputs of the cases, not cases (the package vector's `assets`).
  static let inputs: Set = ["assets"]
  /// Groups of either file that the Swift tests do not run yet, each with the reason. Empty today.
  static let notYetAdopted: Set<String> = []

  /// The groups of a vector file: its keys whose values are lists of cases.
  static func groups(of file: JsonValue) -> Set<String> {
    guard case .object(let fields) = file else { return [] }
    return Set(fields.filter { $0.value.array != nil }.keys).subtracting(inputs)
  }

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

  /// A field the file lacks, or has with another type: the vector is read strictly, so a renamed
  /// field fails here and is not read as a default.
  struct Unreadable: Error, CustomStringConvertible {
    let what: String
    var description: String { "the vector has no \(what)" }
  }

  static func phonemes(_ v: JsonValue) throws -> [AvatarPhoneme] {
    try v.items("phonemes").map {
      let p = try $0.text("p")
      guard p.unicodeScalars.count == 1, let scalar = p.unicodeScalars.first else {
        throw Unreadable(what: "one-scalar phoneme \(p)")
      }
      return AvatarPhoneme(p: scalar, t0: try $0.number("t0"), t1: try $0.number("t1"))
    }
  }

  static func phonemeValues(_ list: [AvatarPhoneme]) -> JsonValue {
    .array(
      list.map {
        .object(["p": .string(String($0.p)), "t0": .number($0.t0), "t1": .number($0.t1)])
      })
  }

  static func envelope(_ v: JsonValue) throws -> AvatarEnvelope {
    AvatarEnvelope(db: try v.items("db").map { try $0.asNumber() }, hop: try v.number("hop"))
  }

  /// `headRadii` of a case: the horizontal and vertical radius of the head, in pixels. A case that
  /// has none is on the reference face.
  static func radii(_ v: JsonValue) throws -> [Double] {
    guard case .array(let given) = v["headRadii"] else { return AvatarActing.referenceHead }
    guard given.count == 2 else { throw Unreadable(what: "two headRadii") }
    return try given.map { try $0.asNumber() }
  }

  static func state(_ id: String) throws -> AvatarState {
    guard let state = AvatarState(rawValue: id) else { throw Unreadable(what: "state \(id)") }
    return state
  }

  static func language(_ id: String) throws -> AvatarLanguage {
    guard ["en", "pt-BR"].contains(id) else { throw Unreadable(what: "language \(id)") }
    return AvatarLanguage(id: id)
  }

  static func seed(_ v: JsonValue) throws -> UInt64 {
    guard case .integer(let n) = v["seed"], n >= 0 else { throw Unreadable(what: "seed") }
    return UInt64(n)
  }

  static func wordStarts(_ v: JsonValue) throws -> [AvatarWordStart] {
    try v.items("words").map {
      AvatarWordStart(
        t: try $0.number("t"), word: try $0.text("word"),
        prevEndsClause: try $0.flag("prevEndsClause"))
    }
  }
}

extension JsonValue {
  func asNumberOrText() throws -> String {
    guard let value = string else { throw AvatarVector.Unreadable(what: "a path in \(self)") }
    return value
  }

  func asNumber() throws -> Double {
    guard let value = double else { throw AvatarVector.Unreadable(what: "a number in \(self)") }
    return value
  }

  func number(_ key: String) throws -> Double {
    guard let value = self[key].double else { throw AvatarVector.Unreadable(what: key) }
    return value
  }

  func integer(_ key: String) throws -> Int {
    guard let value = self[key].int else { throw AvatarVector.Unreadable(what: key) }
    return value
  }

  func text(_ key: String) throws -> String {
    guard let value = self[key].string else { throw AvatarVector.Unreadable(what: key) }
    return value
  }

  func flag(_ key: String) throws -> Bool {
    guard let value = self[key].bool else { throw AvatarVector.Unreadable(what: key) }
    return value
  }

  func items(_ key: String) throws -> [JsonValue] {
    guard let value = self[key].array else { throw AvatarVector.Unreadable(what: key) }
    return value
  }
}
