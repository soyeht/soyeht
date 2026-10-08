// Writes the app icon sets of the Mac and iOS apps. Run it from the repository root with
// `scripts/generate_app_icons.sh`. The mark itself (`SoyehtMark`) and the colours (the default
// theme's palette) come from SoyehtClient, the same code and values the interface uses; only
// the platform canvases are described here.
import AppKit
import ImageIO
import SoyehtClient
import SwiftUI
import UniformTypeIdentifiers

/// The icon canvases, in points of a 1024-point artboard.
private enum IconGrid {
  static let artboard: CGFloat = 1024
  /// Apple's macOS icon grid: an 824-point body centred in the artboard.
  static let macBody: CGFloat = 824
  /// The corner radius of Apple's macOS body shape, as a fraction of the body.
  static let macCornerProportion: CGFloat = 185.4 / 824
  /// How much the approved composition is enlarged about its centre, which crops the canvas
  /// edge and makes the face bigger.
  static let zoom: CGFloat = 1.4
  static let macPixelSizes = [16, 32, 128, 256, 512]
}

/// The approved composition: the canvas with the raised square centred on it, shadows included,
/// scaled by `IconGrid.zoom` about its centre and cropped to `side`. Everything scales from the
/// 220-point original.
private struct ZoomedMark: View {
  let colors: SoyehtThemeColorSet
  let side: CGFloat

  var body: some View {
    let canvas = side * IconGrid.zoom
    ZStack {
      colors.canvas
      SoyehtMark(colors: colors, side: canvas * SoyehtMarkMetrics.faceOfCanvas, style: .icon)
    }
    .frame(width: canvas, height: canvas)
    .frame(width: side, height: side)
  }
}

/// The zoomed composition clipped to Apple's body on a transparent artboard: no shadow or halo
/// outside the body.
private struct MacIcon: View {
  let colors: SoyehtThemeColorSet

  var body: some View {
    ZoomedMark(colors: colors, side: IconGrid.macBody)
      .clipShape(
        RoundedRectangle(
          cornerRadius: IconGrid.macBody * IconGrid.macCornerProportion, style: .continuous)
      )
      .frame(width: IconGrid.artboard, height: IconGrid.artboard)
  }
}

/// iOS rounds the corners itself, so the icon is the zoomed composition, full bleed.
private struct IOSIcon: View {
  let colors: SoyehtThemeColorSet

  var body: some View {
    ZoomedMark(colors: colors, side: IconGrid.artboard)
  }
}

private struct IconFailure: Error, CustomStringConvertible {
  let description: String
}

@MainActor private func render(_ view: some View, pixels: Int, opaque: Bool) throws -> CGImage {
  let renderer = ImageRenderer(content: view)
  renderer.scale = CGFloat(pixels) / IconGrid.artboard
  renderer.isOpaque = opaque
  guard let image = renderer.cgImage, image.width == pixels, image.height == pixels else {
    throw IconFailure(description: "could not render a \(pixels) pixel icon")
  }
  guard opaque else { return image }
  // An App Store icon has no alpha channel: redraw into an RGB context that has none.
  guard
    let context = CGContext(
      data: nil, width: pixels, height: pixels, bitsPerComponent: 8, bytesPerRow: 0,
      space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
      bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
  else { throw IconFailure(description: "could not flatten the \(pixels) pixel icon") }
  context.draw(image, in: CGRect(x: 0, y: 0, width: pixels, height: pixels))
  guard let flattened = context.makeImage() else {
    throw IconFailure(description: "could not flatten the \(pixels) pixel icon")
  }
  return flattened
}

private func writePNG(_ image: CGImage, to url: URL) throws {
  guard
    let destination = CGImageDestinationCreateWithURL(
      url as CFURL, UTType.png.identifier as CFString, 1, nil)
  else { throw IconFailure(description: "cannot write \(url.path)") }
  CGImageDestinationAddImage(destination, image, nil)
  guard CGImageDestinationFinalize(destination) else {
    throw IconFailure(description: "cannot finalize \(url.path)")
  }
}

private func writeContents(_ images: [[String: String]]?, to directory: URL) throws {
  var contents: [String: Any] = ["info": ["author": "xcode", "version": 1]]
  if let images { contents["images"] = images }
  let data = try JSONSerialization.data(
    withJSONObject: contents, options: [.prettyPrinted, .sortedKeys])
  try data.write(to: directory.appendingPathComponent("Contents.json"))
}

/// Recreates an icon set inside its asset catalogue, so a size that is no longer generated
/// cannot linger.
private func freshSet(at directory: URL) throws {
  try? FileManager.default.removeItem(at: directory)
  try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  try writeContents(nil, to: directory.deletingLastPathComponent())
}

@MainActor private func writeMacSet(colors: SoyehtThemeColorSet, to directory: URL) throws {
  try freshSet(at: directory)
  var entries: [[String: String]] = []
  for points in IconGrid.macPixelSizes {
    for scale in [1, 2] {
      let name = "icon_\(points)x\(points)\(scale == 2 ? "@2x" : "").png"
      try writePNG(
        render(MacIcon(colors: colors), pixels: points * scale, opaque: false),
        to: directory.appendingPathComponent(name))
      entries.append([
        "filename": name, "idiom": "mac", "scale": "\(scale)x", "size": "\(points)x\(points)",
      ])
    }
  }
  try writeContents(entries, to: directory)
}

@MainActor private func writeIOSSet(colors: SoyehtThemeColorSet, to directory: URL) throws {
  try freshSet(at: directory)
  let name = "AppIcon.png"
  try writePNG(
    render(IOSIcon(colors: colors), pixels: Int(IconGrid.artboard), opaque: true),
    to: directory.appendingPathComponent(name))
  try writeContents(
    [["filename": name, "idiom": "universal", "platform": "ios", "size": "1024x1024"]],
    to: directory)
}

@MainActor private func run() throws {
  guard CommandLine.arguments.count == 2 else {
    throw IconFailure(description: "usage: SoyehtIconGenerator <repository root>")
  }
  let root = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
  let colors = SoyehtThemeColorSet(palette: TerminalColorTheme.seededDefault.palette)
  try writeMacSet(
    colors: colors,
    to: root.appendingPathComponent("apps/mac/Resources/Assets.xcassets/AppIcon.appiconset"))
  try writeIOSSet(
    colors: colors,
    to: root.appendingPathComponent("apps/ios/Resources/Assets.xcassets/AppIcon.appiconset"))
}

do {
  try run()
} catch {
  FileHandle.standardError.write(Data("generate_app_icons: \(error)\n".utf8))
  exit(1)
}
