import Foundation

/// An avatar package read from its folder: the validated document and its pictures, decoded.
struct AvatarLoadedPackage: Sendable {
  let package: AvatarPackage
  /// The decoded pictures, by the path the package lists them under.
  let images: [String: AvatarBitmap]

  /// A renderer for the package, drawn no larger than `maxSide` pixels.
  func renderer(maxSide: Int = 512) throws -> AvatarRenderer {
    try AvatarRenderer(package: package, images: images, maxSide: maxSide)
  }
}

/// Reads a package folder (`avatar.json` and the PNG files it lists) into memory, with the same
/// bounds the validator states: the document is not read past its limit, a picture is not read
/// before the document that lists it has passed every rule, and a link or a file outside the
/// folder is not followed.
enum AvatarPackageFolder {
  static let documentName = "avatar.json"

  /// Just the paths a document lists, to find the files to measure before it is validated.
  private struct Listed: Decodable {
    struct File: Decodable { var path: String }
    var files: [File]
  }

  /// A regular file inside the folder: not a link, and no larger than `limit` bytes. Anything
  /// else is `nil`, and the validator then names the file that is missing.
  private static func regularFile(at url: URL, limit: UInt64) -> UInt64? {
    do {
      let values = try url.resourceValues(forKeys: [
        .isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey,
      ])
      guard values.isRegularFile == true, values.isSymbolicLink != true,
        let size = values.fileSize, UInt64(size) <= limit
      else { return nil }
      return UInt64(size)
    } catch {
      return nil
    }
  }

  /// The first bytes of a file: enough for the header of a PNG. A file that cannot be read has
  /// no size, and the validator names it.
  private static func header(of url: URL) -> Data? {
    do {
      let handle = try FileHandle(forReadingFrom: url)
      // The handle closes when it is released.
      return try handle.read(upToCount: 24)
    } catch {
      return nil
    }
  }

  /// Read and validate the package in `folder`. A failure is an `AvatarPackageError`.
  static func load(_ folder: URL) throws -> AvatarLoadedPackage {
    let document = folder.appendingPathComponent(documentName)
    guard regularFile(at: document, limit: UInt64(AvatarPackageLimits.documentBytes + 1)) != nil
    else { throw AvatarPackageError(kind: .malformed, at: "") }
    let json: Data
    do { json = try Data(contentsOf: document) } catch {
      throw AvatarPackageError(kind: .malformed, at: "")
    }
    // A document that does not parse is named by the validator below; here it lists no files.
    let paths: [String]
    do { paths = try JSONDecoder().decode(Listed.self, from: json).files.map(\.path) } catch {
      paths = []
    }
    // Measure what the document lists, never reading more than the headers; a path that is not a
    // plain one is left unmeasured and the validator names it.
    var assets: [AvatarAssetInfo] = []
    for path in paths.prefix(AvatarPackageLimits.files + 1)
    where AvatarPackageValidator.isPlainPngPath(path) {
      let url = folder.appendingPathComponent(path)
      guard let bytes = regularFile(at: url, limit: AvatarPackageLimits.fileBytes + 1) else {
        continue
      }
      assets.append(
        AvatarAssetInfo(path: path, bytes: bytes, size: header(of: url).flatMap(avatarPngSize)))
    }
    let package = try AvatarPackage.load(json: json, assets: assets)
    var images: [String: AvatarBitmap] = [:]
    for file in package.files {
      let size = AvatarSize(width: file.width, height: file.height)
      let data: Data
      do { data = try Data(contentsOf: folder.appendingPathComponent(file.path)) } catch {
        throw AvatarPackageError(kind: .missingAsset, at: file.path)
      }
      guard let bitmap = AvatarBitmap(decoding: data, expecting: size) else {
        throw AvatarPackageError(kind: .assetSize, at: file.path)
      }
      images[file.path] = bitmap
    }
    return AvatarLoadedPackage(package: package, images: images)
  }
}
