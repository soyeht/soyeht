#!/usr/bin/env swift
import AppKit
import CoreText
import Foundation

struct FontMetric {
  let name: String
  let pointSize: CGFloat
  let characterWidth: CGFloat
  let lineHeight: CGFloat
}

struct PaneFloor {
  let name: String
  let surfaceWidth: CGFloat
  let surfaceHeight: CGFloat

  var footprintWidth: CGFloat { ceil(surfaceWidth + Measurement.paneHalo * 2) }
  var footprintHeight: CGFloat { ceil(surfaceHeight + Measurement.paneHalo * 2) }
  var minimumWindowHeight: CGFloat {
    ceil(footprintHeight + Measurement.shellVerticalOverhead)
  }
}

enum Measurement {
  static let paneHalo: CGFloat = 15
  static let shellVerticalOverhead: CGFloat = 84
  static let retinaScale: CGFloat = 2

  static func registerFonts() throws {
    let root =
      ProcessInfo.processInfo.environment["SOYEHT_FLOOR_FONT_DIR"].map {
        URL(fileURLWithPath: $0)
      }
      ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
      .appendingPathComponent("apps/mac/Resources/Fonts")
    let urls = try FileManager.default.contentsOfDirectory(
      at: root, includingPropertiesForKeys: nil
    ).filter { $0.pathExtension == "ttf" }
    for url in urls {
      guard CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil) else {
        // CoreText returns false when a font is already registered in this process.
        continue
      }
    }
  }

  static func metric(
    name: String, size: CGFloat, character: String, snappedForTerminal: Bool = false
  ) -> FontMetric {
    guard let font = NSFont(name: name, size: size) else {
      fatalError("Font unavailable: \(name) \(size)")
    }
    let ctFont = font as CTFont
    let rawWidth = (character as NSString).size(withAttributes: [.font: font]).width
    let rawHeight = ceil(
      CTFontGetAscent(ctFont) + CTFontGetDescent(ctFont) + CTFontGetLeading(ctFont))
    if snappedForTerminal {
      return FontMetric(
        name: font.fontName, pointSize: size,
        characterWidth: (rawWidth * retinaScale).rounded() / retinaScale,
        lineHeight: ceil(rawHeight * retinaScale) / retinaScale)
    }
    return FontMetric(
      name: font.fontName, pointSize: size, characterWidth: rawWidth,
      lineHeight: rawHeight)
  }

}

try Measurement.registerFonts()

let terminal = Measurement.metric(
  name: "JetBrains Mono", size: 13, character: "W", snappedForTerminal: true)
let editor = Measurement.metric(name: "JetBrains Mono", size: 11.5, character: "0")
let chrome = Measurement.metric(name: "Nunito", size: 12, character: "0")
let agent = Measurement.metric(name: "Nunito", size: 15, character: "0")
let prose = Measurement.metric(name: "Newsreader", size: 20, character: "0")
let proseTimes = Measurement.metric(name: "Times New Roman", size: 20, character: "0")
let proseNunito = Measurement.metric(name: "Nunito", size: 20, character: "0")
let screenplay = Measurement.metric(name: "Courier Prime", size: 16, character: "0")

let terminalFloor = PaneFloor(
  name: "terminal",
  surfaceWidth: ceil(40 * terminal.characterWidth + 20),
  surfaceHeight: ceil(
    20 + 26 + 8 + (chrome.lineHeight + 14) + 12 * terminal.lineHeight))

let editorFloor = PaneFloor(
  name: "editor",
  surfaceWidth: ceil(36 * editor.characterWidth + 44 + 20 + 20),
  surfaceHeight: 20 + 27 + 8 + 26 + (12 * 19 + 20) + 42 + 24)

let agentFloor = PaneFloor(
  name: "agent",
  surfaceWidth: ceil(24 * agent.characterWidth + 124),
  surfaceHeight: 20 + 26 + 8 + 4 + 34 + 12 + 33 + 1 + 150 + 67)

let webFloor = PaneFloor(
  name: "web", surfaceWidth: 320 + 20,
  surfaceHeight: 20 + 26 + 8 + 27 + 8 + 240 + 8 + 24)

let textFloor = PaneFloor(
  name: "text",
  surfaceWidth: ceil(24 * prose.characterWidth + 32 + 20),
  surfaceHeight: 20 + 28 + 42 + 42 + 3 * 8 + 32 + 6 * 32)

let screenplayPageWidth = 61 * screenplay.characterWidth + 148
let screenplayZoom = 7.5 / screenplay.pointSize
let screenplayCompactWidth = ceil(screenplayPageWidth * screenplayZoom)
let screenplayCompactHeight = ceil(screenplayCompactWidth * 11 / 8.5)
let screenplayFloor = PaneFloor(
  name: "screenplay", surfaceWidth: screenplayCompactWidth,
  surfaceHeight: 42 + 60 + 34 + screenplayCompactHeight)

let presentationFloor = PaneFloor(
  name: "presentation", surfaceWidth: 320 + 20,
  surfaceHeight: 42 + 32 + 180 + 28 + 20)

let newPaneChooserFloor = PaneFloor(
  name: "new-pane-chooser",
  surfaceWidth: ceil(24 * agent.characterWidth + 124),
  surfaceHeight: 20 + 26 + 8 + 24 + 24 + 3 * 18 + 150 + 32 + 64)

let floors = [
  terminalFloor, editorFloor, agentFloor, webFloor, textFloor, screenplayFloor,
  presentationFloor, newPaneChooserFloor,
]

print("fonts")
for metric in [terminal, editor, chrome, agent, prose, proseTimes, proseNunito, screenplay] {
  print(
    "\(metric.name)@\(metric.pointSize): char=\(metric.characterWidth), line=\(metric.lineHeight)")
}
print("screenplay-base-page-width=\(screenplayPageWidth)")
print("screenplay-minimum-zoom=\(screenplayZoom)")
print("floors")
for floor in floors {
  print(
    "\(floor.name): surface=\(Int(floor.surfaceWidth))x\(Int(floor.surfaceHeight)), "
      + "footprint=\(Int(floor.footprintWidth))x\(Int(floor.footprintHeight)), "
      + "window-height=\(Int(floor.minimumWindowHeight))")
}

let widths: [CGFloat] = [375, 390, 430, 768, 1024, 1440, 3440]
let heights: [CGFloat] = [812, 768, 736, 672, 640, 568, 480, 430, 390, 375]
print("matrix")
for floor in floors {
  for width in widths {
    let widthPasses = width >= floor.footprintWidth
    let results = heights.map { height in
      widthPasses && height >= floor.minimumWindowHeight ? "P" : "F"
    }.joined()
    print("\(floor.name),\(Int(width)),\(results)")
  }
}
