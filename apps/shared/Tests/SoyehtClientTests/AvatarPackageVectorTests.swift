import Foundation
import SoyehtAvatarRaster
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/avatar-package.json`, which `soyeht-avatar` passes as well:
/// the load rules of a package, with a case at every limit (loads) and one past it (fails), and
/// the blend and the picture layers of the base package's mouth.
private func vector() throws -> JsonValue { try ContractVector.load("avatar-package") }

// MARK: - the patches of a case

/// Replace `{n}` in every string of a value with `n`.
private func numbered(_ value: JsonValue, _ n: Int) -> JsonValue {
  switch value {
  case .string(let s): .string(s.replacingOccurrences(of: "{n}", with: String(n)))
  case .array(let a): .array(a.map { numbered($0, n) })
  case .object(let o): .object(o.mapValues { numbered($0, n) })
  default: value
  }
}

private func pointer(_ path: String) -> [String] {
  path.split(separator: "/", omittingEmptySubsequences: true).map(String.init)
}

/// Change the value at `path`: `edit` gets it (nil when absent) and returns the new one (nil
/// removes it). A missing parent leaves the document as it is.
private func edited(
  _ doc: JsonValue, _ path: ArraySlice<String>, _ edit: (JsonValue?) -> JsonValue?
) -> JsonValue {
  guard let key = path.first else { return edit(doc) ?? doc }
  switch doc {
  case .object(var o):
    if let next = edited(o[key] ?? .null, path.dropFirst(), edit, present: o[key] != nil) {
      o[key] = next
    } else {
      o[key] = nil
    }
    return .object(o)
  case .array(var a):
    guard let i = Int(key), a.indices.contains(i) else { return doc }
    if let next = edited(a[i], path.dropFirst(), edit, present: true) {
      a[i] = next
    } else {
      a.remove(at: i)
    }
    return .array(a)
  default:
    return doc
  }
}

/// The value of a child after an edit, `nil` when the edit removed it.
private func edited(
  _ child: JsonValue, _ rest: ArraySlice<String>, _ edit: (JsonValue?) -> JsonValue?,
  present: Bool
) -> JsonValue? {
  if rest.isEmpty { return edit(present ? child : nil) }
  return edited(child, rest, edit)
}

private func apply(_ doc: JsonValue, _ patch: JsonValue) throws -> JsonValue {
  let path = pointer(try patch.text("path"))[...]
  // `times` is there for the operations that repeat, and one when it is not.
  let times = patch["times"] == .null ? 1 : try patch.integer("times")
  switch try patch.text("op") {
  case "append":
    return edited(doc, path) { target in
      guard case .array(let a)? = target else { return target }
      return .array(a + (0..<times).map { numbered(patch["value"], $0) })
    }
  case "repeat":
    let pattern = try patch.items("pattern")
    return edited(doc, path) { _ in
      .array(Array((0..<times).map { _ in pattern }.joined()))
    }
  case "remove":
    return edited(doc, path) { _ in nil }
  case "replace":
    return edited(doc, path) { _ in patch["value"] }
  case let other:
    throw AvatarVector.Unreadable(what: "an operation named \(other)")
  }
}

// MARK: - the assets of a case

/// The size of a picture a file holds: two numbers, or `null` for a file that is not a picture.
private func size(_ v: JsonValue) throws -> AvatarSize? {
  if v == .null { return nil }
  guard let s = v.array, s.count == 2, let w = s[0].int, let h = s[1].int else {
    throw AvatarVector.Unreadable(what: "a picture size or null")
  }
  return AvatarSize(width: UInt32(w), height: UInt32(h))
}

private func assetList(_ v: JsonValue) throws -> [AvatarAssetInfo] {
  guard let list = v.array else { throw AvatarVector.Unreadable(what: "a list of assets") }
  return try list.map {
    AvatarAssetInfo(
      path: try $0.text("path"), bytes: UInt64(try $0.integer("bytes")), size: try size($0["size"]))
  }
}

private func assets(_ base: [AvatarAssetInfo], _ change: JsonValue) throws -> [AvatarAssetInfo] {
  // A case that changes nothing has no `assets`; one that does says what, by these keys only.
  let known: Set = ["remove", "size", "bytes", "add"]
  if case .object(let fields) = change, !fields.keys.allSatisfy(known.contains) {
    throw AvatarVector.Unreadable(what: "an asset change with known keys")
  }
  let removed = Set(try (change["remove"].array ?? []).map { try $0.asNumberOrText() })
  var out = try base.filter { !removed.contains($0.path) }.map { asset in
    var asset = asset
    if case .object(let sizes) = change["size"], let s = sizes[asset.path] {
      asset.size = try size(s)
    }
    if case .object(let bytes) = change["bytes"], let n = bytes[asset.path] {
      guard let n = n.int else { throw AvatarVector.Unreadable(what: "a byte count") }
      asset.bytes = UInt64(n)
    }
    return asset
  }
  // `add`: files the caller has beyond the base's, `times` copies each, `{n}` the copy's number.
  for entry in change["add"].array ?? [] {
    for n in 0..<(entry["times"] == .null ? 1 : try entry.integer("times")) {
      out += try assetList(.array([numbered(entry, n)]))
    }
  }
  return out
}

private func outcome(_ base: JsonValue, _ assetsFile: [AvatarAssetInfo], _ item: JsonValue) throws
  -> (kind: String, at: String?)
{
  var doc = base
  for patch in try item.items("patch") { doc = try apply(doc, patch) }
  let encoder = JSONEncoder()
  encoder.outputFormatting = [.withoutEscapingSlashes]
  var data = try encoder.encode(doc)
  // `documentBytes`: blanks follow the JSON until the document is exactly that many bytes.
  if item["documentBytes"] != .null {
    let bytes = try item.integer("documentBytes")
    guard bytes >= data.count else { throw AvatarVector.Unreadable(what: "room for the document") }
    data.append(Data(repeating: 0x20, count: bytes - data.count))
  }
  do {
    _ = try AvatarPackage.load(json: data, assets: try assets(assetsFile, item["assets"]))
    return ("ok", nil)
  } catch let error as AvatarPackageError {
    return (error.kind.rawValue, error.at)
  }
}

@Test func everyLoadCaseOfThePackageVectorGivesTheKindTheFileSays() throws {
  let file = try vector()
  #expect(AvatarVector.packageGroups.contains("cases"))
  let held = try assetList(file["assets"])
  var kinds = Set<String>()
  for item in ContractVector.cases(file, "cases") {
    let want = try item.text("expect")
    let got = try outcome(file["base"], held, item)
    let name = try item.text("name")
    #expect(got.kind == want, "\(name)")
    // A case that says where the rule was broken is held to it. None of the cases does today: the
    // file pins the kind, and the pointer is the reader's own.
    if item["at"] != .null {
      let at = try item.text("at")
      #expect(got.at == at, "\(name)")
    }
    kinds.insert(want)
  }
  // Every failure kind the loader has is exercised by a case.
  for kind in [
    "malformed", "wrongFormat", "unsupportedVersion", "canvas", "file", "missingAsset",
    "assetSize", "tooLarge", "shapes", "topology", "index", "value", "frames", "warp", "acting",
    "atlas", "ok",
  ] {
    #expect(kinds.contains(kind), "no case for \(kind)")
  }
}

@Test func theBlendAndTheLayersOfTheMouthAreTheVectors() throws {
  let file = try vector()
  #expect(AvatarVector.packageGroups.contains("mesh"))
  let held = try assetList(file["assets"])
  let package = try AvatarPackage.load(
    json: JSONEncoder().encode(file["base"]), assets: held)
  for item in ContractVector.cases(file, "mesh") {
    let weights = try item.items("weights").map { try $0.asNumber() }
    let parts: [JsonValue] = package.mouth.parts.map { part in
      .object([
        "id": .string(part.id.rawValue),
        "positions": AvatarVector.numbers(part.blend(weights)),
        "opacity": .number(part.opacity(weights)),
        "layers": .array(
          part.layers(weights).map {
            .object(["shape": .string($0.shape.id), "alpha": .number($0.alpha)])
          }),
      ])
    }
    let why = AvatarVector.difference(
      .object(["parts": .array(parts)]), item["expected"], tolerance: 1e-3)
    let name = try item.text("name")
    #expect(why == nil, "mesh / \(name): \(why ?? "")")
  }
}

@Test func theDrawingOrderOfTheMouthsPartsIsTheOneTheVectorsName() throws {
  #expect(AvatarVector.packageGroups.contains("drawOrder"))
  let file = try vector()
  let package = try AvatarPackage.load(
    json: JSONEncoder().encode(file["base"]), assets: try assetList(file["assets"]))
  for item in ContractVector.cases(file, "drawOrder") {
    let listed = try item.items("parts").map { try $0.asNumberOrText() }
    var model = package.mouth
    model.parts = try listed.map { name in
      guard let part = package.mouth.parts.first(where: { $0.id.rawValue == name }) else {
        throw AvatarVector.Unreadable(what: "a part named \(name)")
      }
      return part
    }
    let order = AvatarVector.stringList(model.inDrawingOrder.map(\.id.rawValue))
    let name = try item.text("name")
    let why = AvatarVector.difference(
      .object(["order": order]), item["expected"], tolerance: 1e-3)
    #expect(why == nil, "drawOrder / \(name): \(why ?? "")")
  }
}

private func colour(_ v: JsonValue) throws -> SIMD4<Float> {
  guard let list = v.array, list.count == 4 else { throw AvatarVector.Unreadable(what: "a colour") }
  let n = try list.map { Float(try $0.asNumber()) }
  return SIMD4(n[0], n[1], n[2], n[3])
}

@Test func aPartLaidOverAPictureIsTheColourTheVectorsName() throws {
  #expect(AvatarVector.packageGroups.contains("composite"))
  var results: [String: SIMD4<Float>] = [:]
  for item in ContractVector.cases(try vector(), "composite") {
    // The layers are mixed by their weights, each over the mix before it, from a clear mix, and
    // the mix is laid over what is under it once: the two steps the rasteriser's passes are made of.
    var mix = SIMD4<Float>.zero
    for layer in try item.items("layers") {
      mix = AvatarRasterizer.mixStep(
        mix, texel: try colour(layer["texel"]), alpha: Float(try layer.number("alpha")))
    }
    let out = AvatarRasterizer.compose(
      mix: mix, under: try colour(item["under"]), coverage: Float(try item.number("coverage")))
    let name = try item.text("name")
    let got = AvatarVector.numbers((0..<4).map { Double(out[$0]) })
    let why = AvatarVector.difference(
      .object(["colour": got]), item["expected"], tolerance: 1e-3)
    #expect(why == nil, "composite / \(name): \(why ?? "")")
    results[name] = out
  }
  // A drawing that is all but absent changes the pixel by under a 255th.
  let pairs = [
    (
      "a clear texel with all the weight, the drawing before it under the threshold",
      "the same with the drawing before it over the threshold"
    ),
    ("a feathered pixel, one layer", "the same with a second layer at the weight 0.0002"),
  ]
  for (one, two) in pairs {
    let (a, b) = (try #require(results[one]), try #require(results[two]))
    #expect((0..<4).allSatisfy { abs(a[$0] - b[$0]) < 1 }, "\(one)")
  }
}
