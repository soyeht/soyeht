#if os(macOS)
  import CoreText
  import Foundation
  import Testing

  @testable import SoyehtClient

  /// The Mac app bundles the faces it draws with instead of relying on what the machine has: a
  /// preview tab is italic, and on a Mac without JetBrains Mono installed the system has no italic
  /// of it, so the marker vanished and a preview looked like a tab that had been replaced.
  private func bundledFonts(file: String = #filePath) throws -> [URL] {
    let fonts = URL(fileURLWithPath: file)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      .deletingLastPathComponent().appendingPathComponent("mac/Resources/Fonts")
    return try FileManager.default.contentsOfDirectory(at: fonts, includingPropertiesForKeys: nil)
      .filter { $0.pathExtension == "ttf" }
  }

  @Test func theBundledMonoFamilyHasAnItalicFaceForThePreviewTab() throws {
    let faces = try bundledFonts().flatMap { url -> [CTFontDescriptor] in
      (CTFontManagerCreateFontDescriptorsFromURL(url as CFURL) as? [CTFontDescriptor]) ?? []
    }
    let italicMono = faces.filter { face in
      let family = CTFontDescriptorCopyAttribute(face, kCTFontFamilyNameAttribute) as? String
      let traits = CTFontDescriptorCopyAttribute(face, kCTFontTraitsAttribute) as? [CFString: Any]
      let symbolic = (traits?[kCTFontSymbolicTrait] as? NSNumber)?.uint32Value ?? 0
      return family == PaneInteriorFont.monoFamily
        && symbolic & CTFontSymbolicTraits.traitItalic.rawValue != 0
    }
    #expect(!italicMono.isEmpty)
  }
#endif
