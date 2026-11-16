import Foundation
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

private func apply(_ doc: JsonValue, _ patch: JsonValue) -> JsonValue {
  let path = pointer(patch["path"].string ?? "")[...]
  let times = patch["times"].int ?? 1
  switch patch["op"].string {
  case "append":
    return edited(doc, path) { target in
      guard case .array(let a)? = target else { return target }
      return .array(a + (0..<times).map { numbered(patch["value"], $0) })
    }
  case "repeat":
    let pattern = patch["pattern"].array ?? []
    return edited(doc, path) { _ in
      .array(Array((0..<times).map { _ in pattern }.joined()))
    }
  case "remove":
    return edited(doc, path) { _ in nil }
  default:
    return edited(doc, path) { _ in patch["value"] }
  }
}

// MARK: - the assets of a case

private func size(_ v: JsonValue) -> AvatarSize? {
  guard let s = v.array, s.count == 2, let w = s[0].int, let h = s[1].int else { return nil }
  return AvatarSize(width: UInt32(w), height: UInt32(h))
}

private func assetList(_ v: JsonValue) -> [AvatarAssetInfo] {
  (v.array ?? []).map {
    AvatarAssetInfo(
      path: $0["path"].string ?? "", bytes: UInt64($0["bytes"].int ?? 0), size: size($0["size"]))
  }
}

private func assets(_ base: [AvatarAssetInfo], _ change: JsonValue) -> [AvatarAssetInfo] {
  let removed = Set((change["remove"].array ?? []).compactMap(\.string))
  var out = base.filter { !removed.contains($0.path) }.map { asset in
    var asset = asset
    if case .object(let sizes) = change["size"], let s = sizes[asset.path] {
      asset.size = size(s)
    }
    if let bytes = change["bytes"][asset.path].int { asset.bytes = UInt64(bytes) }
    return asset
  }
  // `add`: files the caller has beyond the base's, `times` copies each, `{n}` the copy's number.
  for entry in change["add"].array ?? [] {
    for n in 0..<(entry["times"].int ?? 1) {
      out += assetList(.array([numbered(entry, n)]))
    }
  }
  return out
}

private func outcome(_ base: JsonValue, _ assetsFile: [AvatarAssetInfo], _ item: JsonValue) throws
  -> String
{
  var doc = base
  for patch in item["patch"].array ?? [] { doc = apply(doc, patch) }
  let encoder = JSONEncoder()
  encoder.outputFormatting = [.withoutEscapingSlashes]
  var data = try encoder.encode(doc)
  // `documentBytes`: blanks follow the JSON until the document is exactly that many bytes.
  if let bytes = item["documentBytes"].int, bytes > data.count {
    data.append(Data(repeating: 0x20, count: bytes - data.count))
  }
  do {
    _ = try AvatarPackage.load(json: data, assets: assets(assetsFile, item["assets"]))
    return "ok"
  } catch let error as AvatarPackageError {
    return error.kind.rawValue
  }
}

@Test func everyLoadCaseOfThePackageVectorGivesTheKindTheFileSays() throws {
  let file = try vector()
  let held = assetList(file["assets"])
  var kinds = Set<String>()
  for item in ContractVector.cases(file, "cases") {
    let want = item["expect"].string ?? "?"
    let got = try outcome(file["base"], held, item)
    #expect(got == want, "\(item["name"].string ?? "")")
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
  let held = assetList(file["assets"])
  let package = try AvatarPackage.load(
    json: JSONEncoder().encode(file["base"]), assets: held)
  for item in ContractVector.cases(file, "mesh") {
    let weights = (item["weights"].array ?? []).map { $0.double ?? 0 }
    let parts: [JsonValue] = package.mouth.parts.map { part in
      .object([
        "id": .string(part.id.rawValue),
        "positions": AvatarVector.numbers(part.blend(weights)),
        "layers": .array(
          part.layers(weights).map {
            .object(["shape": .string($0.shape.id), "alpha": .number($0.alpha)])
          }),
      ])
    }
    let why = AvatarVector.difference(
      .object(["parts": .array(parts)]), item["expected"], tolerance: 1e-3)
    #expect(why == nil, "mesh / \(item["name"].string ?? ""): \(why ?? "")")
  }
}
