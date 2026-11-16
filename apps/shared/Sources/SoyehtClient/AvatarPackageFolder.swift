import Darwin
import Foundation
import SoyehtAvatarRaster

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

/// Reads a package folder (`avatar.json` and the PNG files it lists) into memory, with the bounds
/// the validator states. Each file is opened once, never through a link (not in its name and not
/// in any directory above it inside the folder), measured with `fstat`, and read up to its limit:
/// the bytes that are measured are the bytes that are decoded, and a file larger than its limit
/// is reported with its true size and never read.
enum AvatarPackageFolder {
  static let documentName = "avatar.json"

  /// Just the paths a document lists, to find the files to measure before it is validated.
  private struct Listed: Decodable {
    struct File: Decodable { var path: String }
    var files: [File]
  }

  /// A file as it is on disk: its true size, the size of the picture its header declares, and its
  /// bytes when they are within the limit.
  private struct Found {
    let size: UInt64
    let picture: AvatarSize?
    let data: Data?
  }

  /// Open `path` inside the folder (a descriptor of the folder itself) without following a link,
  /// and read it when it is a regular file of at most `limit` bytes. `nil` when it cannot be
  /// opened or is not a regular file: the validator then names the file that is missing.
  private static func read(_ path: String, in folder: Int32, limit: Int) -> Found? {
    // `O_NONBLOCK`: opening a named pipe for reading waits for a writer, and a package folder is
    // not trusted not to hold one. The kind of file is checked after the open.
    let descriptor = openat(folder, path, O_RDONLY | O_NOFOLLOW_ANY | O_NONBLOCK)
    guard descriptor >= 0 else { return nil }
    defer { close(descriptor) }
    var info = stat()
    guard fstat(descriptor, &info) == 0, (info.st_mode & S_IFMT) == S_IFREG, info.st_size >= 0
    else { return nil }
    let size = UInt64(info.st_size)
    var header = Data(count: 24)
    let headerCount = header.withUnsafeMutableBytes { pread(descriptor, $0.baseAddress, 24, 0) }
    let picture = headerCount == 24 ? avatarPngSize(header) : nil
    guard size <= UInt64(limit) else { return Found(size: size, picture: picture, data: nil) }
    var data = Data(count: Int(size))
    // A read may return less than asked; the loop goes on until the file has given what it was
    // measured to hold. A file that ends sooner, or fails, is not the file that was measured.
    var have = 0
    let total = Int(size)
    while have < total {
      let count = data.withUnsafeMutableBytes { buffer in
        Darwin.read(descriptor, buffer.baseAddress.map { $0 + have }, total - have)
      }
      if count > 0 {
        have += count
      } else if count < 0 && errno == EINTR {
        continue
      } else {
        return nil
      }
    }
    return Found(size: size, picture: picture, data: data)
  }

  /// Read and validate the package in `folder`. A failure is an `AvatarPackageError`.
  static func load(_ folder: URL) throws -> AvatarLoadedPackage {
    let root = open(folder.path, O_RDONLY | O_DIRECTORY)
    guard root >= 0 else { throw AvatarPackageError(kind: .malformed, at: "") }
    defer { close(root) }
    guard let document = read(documentName, in: root, limit: AvatarPackageLimits.documentBytes)
    else { throw AvatarPackageError(kind: .malformed, at: "") }
    guard let json = document.data else { throw AvatarPackageError(kind: .tooLarge, at: "") }
    // A document that does not parse is named by the validator below; here it lists no files.
    let paths: [String]
    do { paths = try JSONDecoder().decode(Listed.self, from: json).files.map(\.path) } catch {
      paths = []
    }
    // Read what the document lists, at most as many files and bytes as a package may hold; a path
    // that is not a plain one is left alone and the validator names it.
    var found: [String: Found] = [:]
    var assets: [AvatarAssetInfo] = []
    var budget = Int(AvatarPackageLimits.totalBytes)
    for path in paths.prefix(AvatarPackageLimits.files + 1)
    where AvatarPackageValidator.isPlainPngPath(path) && found[path] == nil {
      guard let file = read(path, in: root, limit: min(Int(AvatarPackageLimits.fileBytes), budget))
      else { continue }
      budget -= file.data?.count ?? 0
      found[path] = file
      // A file left unread is reported by its true size and the header's picture size: the
      // validator's own rules say whether it is too large.
      assets.append(AvatarAssetInfo(path: path, bytes: file.size, size: file.picture))
    }
    let package = try AvatarPackage.load(json: json, assets: assets)
    var images: [String: AvatarBitmap] = [:]
    for file in package.files {
      guard let data = found[file.path]?.data,
        let bitmap = AvatarBitmap(
          decoding: data, width: Int(file.width), height: Int(file.height))
      else { throw AvatarPackageError(kind: .assetSize, at: file.path) }
      images[file.path] = bitmap
    }
    return AvatarLoadedPackage(package: package, images: images)
  }
}
